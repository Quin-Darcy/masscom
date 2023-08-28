use std::collections::HashMap;
use std::io::{Read, Write, Error};
use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

fn handle_client(addr: String, stream: TcpStream, command_receiver: mpsc::Receiver<String>, response_sender: mpsc::Sender<String>) -> Result<(), Error> {
    //println!("Incoming connection from: {}", addr);
    let mut stream = stream;
    let mut buf = [0; 512];

    loop {
        //println!("client thread: Beginning of loop in handle_client");
        // Attempt to receive command 

        if let Ok(command) = command_receiver.try_recv() {
            //println!("client thread: Command received");
            //println!("client thread: writing command to client");
            stream.write(command.as_bytes())?;
        }

        //println!("client thread: Reading response from client");
        stream.set_read_timeout(Some(Duration::new(1, 0))).expect("set_read_timeout call failed");

        let result = stream.read(&mut buf);
        match result {
            Ok(bytes_read) => {
                if bytes_read > 0 {
                    // Send response back to main through response channel
                    let response = std::str::from_utf8(&buf[..bytes_read]).expect("Could not write buffer as string");
                    let client_response = format!("[Client {}] {}", addr, response);
                    response_sender.send(client_response).unwrap();
                } else {
                    break;
                }
            }
            Err(error) => {
                match error.kind() {
                    std::io::ErrorKind::WouldBlock => { continue; }
                    std::io::ErrorKind::TimedOut => { continue; }
                    _ => { break; }
                }
            }
        }
    }

    println!("{} - Disconnected", addr);
    Ok(())
}


fn main() {
    let listener = TcpListener::bind("0.0.0.0:8888").expect("Could not bind");

    // The client_map is a hashmap that manages the active client connections.
    // It is keyed by the client's address (as a String) and contains a tuple
    // consisting of:
    // - An mpsc::Sender<String> for the main-to-client channel, allowing the main
    //   thread to send specific commands to individual clients.
    // - A TcpStream, representing the connection to that specific client.
    // The client_map is wrapped in an Arc and Mutex to allow safe concurrent
    // access from multiple threads, ensuring that updates to the client connections
    // are coordinated across the main and client-handling threads.
    let client_map: Arc<Mutex<HashMap<String, (mpsc::Sender<String>, TcpStream)>>> = Arc::new(Mutex::new(HashMap::new()));

    // Clone the Arc before the closure where it's moved
    let client_map_clone = Arc::clone(&client_map);

    // Create the client-to-main (response) channel where the tx portion will be cloned
    // into each of the client threads and the rx portion will be the singular
    // consumer for each of these senders
    let (client_thread_response_sender, main_thread_response_receiver) = mpsc::channel();

    // Add a channel for disconnection notifications
    let (disconnection_sender, disconnection_receiver) = mpsc::channel();

    // Create a thread to listen for incoming connections
    thread::spawn(move || {
        // Accept connections and process them, spawning a new thread for each one
        for stream in listener.incoming() {
            match stream {
                Err(e) => eprintln!("failed: {}", e),
                Ok(stream) => {
                    // Get address of client of client as a String
                    let addr = stream.peer_addr().unwrap().to_string();

                    println!("{} - Connected", addr);

                    // Create individual main-to-client (command) channel
                    let (main_thread_command_sender, client_thread_command_receiver) = mpsc::channel();

                    // Clone the original response sender whose ownership will be moved into client thread
                    let client_thread_response_sender = client_thread_response_sender.clone();

                    // Clone the disconnection channel's sender to give to each client thread
                    let disconnect_sender = disconnection_sender.clone();

                    // Create reference to the shared client_map 
                    let client_map = Arc::clone(&client_map);

                    // Add new entry to the hashmap including the sender for the command channel and the client's TcpStream
                    client_map.lock().unwrap().insert(addr.clone(), (main_thread_command_sender, stream.try_clone().unwrap()));

                    // Handle the client and move ownership of values
                    thread::spawn(move || {
                        let addr_clone = addr.clone();
                        if handle_client(addr, stream, client_thread_command_receiver, client_thread_response_sender).is_err() {
                            // An error has been returned meaning the client has disconnected
                            // Send notification of this event to the disconnection channel's receiver
                            disconnect_sender.send(addr_clone).unwrap();
                        }
                    });
                }
            }
        }
    });

    // Create stdin channel whose sender will be given to stdin thread
    let (stdin_sender, stdin_receiver) = mpsc::channel();

    // Create new thread dedicated to waiting for command line input
    thread::spawn(move || {
        loop {
            let mut command: String = String::new();
            std::io::stdin().read_line(&mut command).expect("Failed to read from stdin");
            stdin_sender.send(command).unwrap();
        }
    });

    // Main loop for handling commands and responses
    loop {
        // Check if stdin thread has read in command
        let mut command: String = String::new();
        if let Ok(stdin_command) = stdin_receiver.try_recv() {
            command = stdin_command;
        }

        // Check for and handle disconnections on the disconnection channel and perform clean-up
        {
            // Create a scope for the lock to be released after this block
            let mut client_map_guard = client_map_clone.lock().unwrap();
            if let Ok(disconnected_addr) = disconnection_receiver.try_recv() {
                // Remove the disconnected client from the client_map
                client_map_guard.remove(&disconnected_addr);
            }
        }

        let client_map = client_map_clone.lock().unwrap();

        // Only broadcast if command is not empty
        if !command.is_empty() {
            println!("[Server Command]: {}", command);
            // Broadcast command to clients
            for (main_thread_command_sender, _) in client_map.values() {
                // Check if send is successful, if not, client is likely disconnected
                if main_thread_command_sender.send(command.clone()).is_err() {
                    continue;
                }
            }
        }

        // Print responses from clients, using non-blocking receive
        let mut received_responses = 0;
        while received_responses < client_map.len() {
            if let Ok(response) = main_thread_response_receiver.try_recv() {
                println!("{}", response);
                received_responses += 1;
            } else {
                // If no responses are available, just break the loop
                break;
            }
        }
    }
}


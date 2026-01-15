//! Interactive example for testing claude-app-server manually
//!
//! Run with: cargo run --example interactive
//!
//! This example demonstrates sending JSON-RPC messages to the server.

use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

fn main() -> io::Result<()> {
    println!("Claude App Server Interactive Test");
    println!("===================================");
    println!();
    println!("This will start the server and let you send JSON-RPC messages.");
    println!("Type 'quit' to exit.");
    println!();

    // Example messages
    let examples = vec![
        (
            "Initialize",
            r#"{"id": 1, "method": "initialize", "params": {}}"#,
        ),
        (
            "List models",
            r#"{"id": 2, "method": "model/list", "params": {}}"#,
        ),
        (
            "Start thread",
            r#"{"id": 3, "method": "thread/start", "params": {"name": "Test"}}"#,
        ),
        (
            "List threads",
            r#"{"id": 4, "method": "thread/list", "params": {}}"#,
        ),
    ];

    println!("Example messages:");
    for (i, (name, msg)) in examples.iter().enumerate() {
        println!("  {}: {} - {}", i + 1, name, msg);
    }
    println!();

    // Start the server process
    let mut child = Command::new("cargo")
        .args(["run", "--release"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    let stdout = child.stdout.take().expect("Failed to open stdout");

    // Spawn thread to read responses
    let handle = std::thread::spawn(move || {
        let reader = io::BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) if !l.is_empty() => {
                    // Pretty print JSON
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&l) {
                        println!(
                            "\n<<< Response:\n{}",
                            serde_json::to_string_pretty(&json).unwrap_or(l)
                        );
                    } else {
                        println!("\n<<< {}", l);
                    }
                    print!("> ");
                    io::stdout().flush().ok();
                }
                Err(e) => {
                    eprintln!("Error reading: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });

    // Read input and send to server
    let stdin_reader = io::stdin();
    print!("> ");
    io::stdout().flush()?;

    for line in stdin_reader.lock().lines() {
        let line = line?;
        if line.trim() == "quit" {
            break;
        }

        // Check for example shortcuts
        if let Ok(num) = line.trim().parse::<usize>() {
            if num > 0 && num <= examples.len() {
                let msg = examples[num - 1].1;
                println!(">>> Sending: {}", msg);
                writeln!(stdin, "{}", msg)?;
                stdin.flush()?;
                continue;
            }
        }

        if !line.trim().is_empty() {
            println!(">>> Sending: {}", line);
            writeln!(stdin, "{}", line)?;
            stdin.flush()?;
        }

        print!("> ");
        io::stdout().flush()?;
    }

    drop(stdin);
    child.kill().ok();
    handle.join().ok();

    println!("\nGoodbye!");
    Ok(())
}

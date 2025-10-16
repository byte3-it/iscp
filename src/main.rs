use console::style;
use dialoguer::{Input, Password};
use indicatif::{ProgressBar, ProgressStyle};
use ssh2::Session;
use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process;

#[derive(Debug)]
struct TransferConfig {
    local_file: String,
    remote_host: String,
    port: u16,
    remote_path: String,
    username: String,
}

fn main() {
    println!("{}", style("=====================================").cyan());
    println!(
        "{}",
        style("🔐 Interactive SCP File Transfer Tool").bold().cyan()
    );
    println!("{}", style("=====================================").cyan());

    // Get transfer configuration from user
    let config = get_transfer_config();

    // Perform the file transfer
    match transfer_file(&config) {
        Ok(_) => {
            println!(
                "\n{}",
                style("✅ File transfer completed successfully!")
                    .green()
                    .bold()
            );
        }
        Err(e) => {
            eprintln!("\n{}", style("❌ Transfer failed:").red().bold());
            eprintln!("{}", style(e).red());
            process::exit(1);
        }
    }
}

fn get_transfer_config() -> TransferConfig {
    // Get local file path
    let local_file: String = Input::new()
        .with_prompt("📁 Local file path")
        .interact()
        .expect("Failed to read local file path");

    if !Path::new(&local_file).exists() {
        eprintln!("{}", style("❌ Local file does not exist!").red().bold());
        process::exit(1);
    }

    // Get remote host
    let remote_host: String = Input::new()
        .with_prompt("🌐 Remote host (e.g., example.com or 192.168.1.100)")
        .interact()
        .expect("Failed to read remote host");

    // Get port (optional)
    let port_input: String = Input::new()
        .with_prompt("🔌 Port (optional, press Enter for default 22)")
        .allow_empty(true)
        .interact()
        .expect("Failed to read port");

    let port = if port_input.is_empty() {
        22
    } else {
        port_input.parse::<u16>().unwrap_or_else(|_| {
            eprintln!(
                "{}",
                style("❌ Invalid port number, using default 22").yellow()
            );
            22
        })
    };

    // Get username
    let username: String = Input::new()
        .with_prompt("👤 Username")
        .interact()
        .expect("Failed to read username");

    // Get remote path (optional)
    let local_filename = Path::new(&local_file)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let default_remote_path = format!("/home/{}/{}", username, local_filename);

    let remote_path: String = Input::new()
        .with_prompt(&format!(
            "📂 Remote path (optional, press Enter for default: {})",
            default_remote_path
        ))
        .allow_empty(true)
        .interact()
        .expect("Failed to read remote path");

    let final_remote_path = if remote_path.is_empty() {
        default_remote_path
    } else {
        remote_path
    };

    TransferConfig {
        local_file,
        remote_host,
        port,
        remote_path: final_remote_path,
        username,
    }
}

fn transfer_file(config: &TransferConfig) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n{}", style("🔗 Connecting to remote host...").blue());

    // Establish TCP connection
    let tcp = TcpStream::connect(format!("{}:{}", config.remote_host, config.port))?;
    let mut sess = Session::new()?;
    sess.set_tcp_stream(tcp);
    sess.handshake()?;

    // Try to authenticate
    if !authenticate(&mut sess, config)? {
        return Err("Authentication failed".into());
    }

    println!(
        "{}",
        style("✅ Connected and authenticated successfully!").green()
    );
    println!("{}", style("📤 Starting file transfer...").blue());

    // Read local file
    let mut local_file = File::open(&config.local_file)?;
    let file_size = local_file.metadata()?.len();

    // Create progress bar
    let progress_bar = ProgressBar::new(file_size);
    progress_bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );

    // Create SCP channel for file transfer
    let mut channel = sess.scp_send(Path::new(&config.remote_path), 0o644, file_size, None)?;

    // Transfer file with progress tracking
    let mut buffer = [0; 8192];
    let mut transferred = 0;

    loop {
        let bytes_read = local_file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }

        channel.write_all(&buffer[..bytes_read])?;
        transferred += bytes_read as u64;
        progress_bar.set_position(transferred);
    }

    // Close the channel
    channel.send_eof()?;
    channel.wait_eof()?;
    channel.close()?;
    channel.wait_close()?;

    progress_bar.finish_with_message("Transfer completed!");

    Ok(())
}

fn authenticate(
    sess: &mut Session,
    config: &TransferConfig,
) -> Result<bool, Box<dyn std::error::Error>> {
    // First, try to authenticate with SSH keys
    if let Ok(home) = std::env::var("HOME") {
        let key_paths = [
            format!("{}/.ssh/id_rsa", home),
            format!("{}/.ssh/id_ed25519", home),
            format!("{}/.ssh/id_ecdsa", home),
        ];

        for key_path in &key_paths {
            if Path::new(key_path).exists() {
                println!(
                    "{}",
                    style(format!("🔑 Trying SSH key: {}", key_path)).blue()
                );

                let key_path = Path::new(key_path);

                // Try without passphrase first
                if let Ok(_) = sess.userauth_pubkey_file(&config.username, None, &key_path, None) {
                    println!(
                        "{}",
                        style("✅ Authenticated with SSH key (no passphrase)").green()
                    );
                    return Ok(true);
                }

                // Key requires passphrase
                println!("{}", style("🔐 SSH key requires passphrase").yellow());
                let passphrase: String = Password::new()
                    .with_prompt("🔑 SSH key passphrase")
                    .interact()
                    .expect("Failed to read passphrase");

                if let Ok(_) = sess.userauth_pubkey_file(
                    &config.username,
                    None,
                    &key_path,
                    Some(passphrase.as_str()),
                ) {
                    println!(
                        "{}",
                        style("✅ Authenticated with SSH key (with passphrase)").green()
                    );
                    return Ok(true);
                }
            }
        }
    }

    // Fallback to password authentication
    println!(
        "{}",
        style("🔐 SSH key authentication failed, trying password authentication").yellow()
    );

    let password: String = Input::new()
        .with_prompt("🔑 Password")
        .interact()
        .expect("Failed to read password");

    match sess.userauth_password(&config.username, &password) {
        Ok(_) => {
            println!("{}", style("✅ Authenticated with password").green());
            Ok(true)
        }
        _ => {
            println!("{}", style("❌ Password authentication failed").red());
            Ok(false)
        }
    }
}

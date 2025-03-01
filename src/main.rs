use anyhow::{Result, Context};
use clap::{Parser, Subcommand};
use ssh2::Session;
use std::net::TcpStream;
use std::io::{Read, Write};
use prettytable::{Table, row};
use ratatui::{
    prelude::*,
    widgets::*,
    Terminal,
    text::{Line, Text}, // Add Text import
};
use crossterm::{
    event::{self, KeyCode, Event},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use std::time::{Duration, Instant};
use humansize::{format_size, BINARY};

#[derive(Parser)]
#[command(name = "remote_management")]
#[command(about = "A CLI tool for remote server management")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Status {
        #[arg(short = 'H', long)]
        host: String,
        #[arg(short, long)]
        username: Option<String>,
        #[arg(short = 'P', long, default_value = "22")]
        port: u16,
    },
    Monitor {
        #[arg(short = 'H', long)]
        host: String,
        #[arg(short, long)]
        username: Option<String>,
        #[arg(short = 'P', long, default_value = "22")]
        port: u16,
        #[arg(short = 'i', long, default_value = "1")]
        interval: u64,
    },
}

fn get_credentials(username: Option<String>) -> Result<(String, String)> {
    let username = match username {
        Some(u) => u,
        None => {
            print!("Enter username: ");
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            input.trim().to_string()
        }
    };
    
    let password = rpassword::prompt_password("Enter password: ")?;
    Ok((username, password))
}

fn get_server_status(host: &str, port: u16, username: Option<String>) -> Result<String> {
    let address = format!("{}:{}", host, port);
    let tcp = TcpStream::connect(&address)
        .with_context(|| format!("Failed to connect to {}", address))?;
    
    let mut sess = Session::new()?;
    sess.set_tcp_stream(tcp);
    sess.handshake()?;

    // Try SSH agent first
    if let Some(user) = &username {
        if sess.userauth_agent(user).is_ok() {
            return get_system_info(&mut sess);
        }
    }

    // If SSH agent fails or no username provided, prompt for credentials
    let (username, password) = get_credentials(username)?;
    sess.userauth_password(&username, &password)
        .with_context(|| "Authentication failed")?;

    get_system_info(&mut sess)
}

fn get_system_info(sess: &mut Session) -> Result<String> {
    let commands = vec![
        "uptime",
        "free -h",
        "df -h",
        "top -bn1 | head -n 3",
    ];

    let mut table = Table::new();
    table.add_row(row!["Metric", "Value"]);

    for cmd in commands {
        let mut channel = sess.channel_session()?;
        channel.exec(cmd)?;
        let mut output = String::new();
        channel.read_to_string(&mut output)?;
        table.add_row(row![cmd, output.trim()]);
        channel.wait_close()?;
    }

    Ok(table.to_string())
}

#[derive(Default)]
struct SystemStats {
    cpu_usage: f64,
    cpu_history: Vec<f64>,
    memory_total: u64,
    memory_used: u64,
    swap_total: u64,
    swap_used: u64,
    disk_usage: Vec<(String, u64, u64)>, // (mount point, total, used)
    load_average: (f64, f64, f64),
    uptime: String,
}

impl SystemStats {
    fn update_cpu_history(&mut self) {
        const MAX_HISTORY: usize = 100;
        if self.cpu_history.len() >= MAX_HISTORY {
            self.cpu_history.remove(0);
        }
        self.cpu_history.push(self.cpu_usage);
    }
}

fn parse_system_stats(output: &str) -> SystemStats {
    let mut stats = SystemStats::default();
    
    // Parse CPU usage from top
    if let Some(cpu_line) = output.lines().find(|l| l.contains("%Cpu(s)")) {
        let parts: Vec<&str> = cpu_line.split_whitespace().collect();
        // Look for the "us," (user CPU usage) value
        for (i, part) in parts.iter().enumerate() {
            if *part == "us," && i > 0 {
                if let Ok(user_cpu) = parts[i - 1].parse::<f64>() {
                    // User CPU percentage + System CPU percentage (if available)
                    stats.cpu_usage = user_cpu;
                    // Try to find system CPU usage
                    if let Some(sys_idx) = parts.iter().position(|p| *p == "sy,") {
                        if let Ok(sys_cpu) = parts[sys_idx - 1].parse::<f64>() {
                            stats.cpu_usage += sys_cpu;
                        }
                    }
                    break;
                }
            }
        }
    }
    
    // Parse memory usage from free
    for line in output.lines() {
        if line.starts_with("Mem:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                stats.memory_total = parts[1].parse().unwrap_or(0);
                stats.memory_used = parts[2].parse().unwrap_or(0);
            }
        } else if line.starts_with("Swap:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                stats.swap_total = parts[1].parse().unwrap_or(0);
                stats.swap_used = parts[2].parse().unwrap_or(0);
            }
        }
    }

    // Parse load average from uptime
    if let Some(uptime_line) = output.lines().find(|l| l.contains("load average:")) {
        if let Some(load_str) = uptime_line.split("load average:").nth(1) {
            let loads: Vec<f64> = load_str
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            if loads.len() >= 3 {
                stats.load_average = (loads[0], loads[1], loads[2]);
            }
        }
        stats.uptime = uptime_line.to_string();
    }

    // Parse disk usage from df
    for line in output.lines() {
        if line.starts_with('/') {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 6 {
                let total: u64 = parts[1].parse().unwrap_or(0);
                let used: u64 = parts[2].parse().unwrap_or(0);
                stats.disk_usage.push((parts[5].to_string(), total, used));
            }
        }
    }

    stats
}

async fn monitor_system(sess: &mut Session, interval: u64) -> Result<()> {
    enable_raw_mode()?;
    std::io::stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;

    let mut last_update = Instant::now();
    let mut stats = SystemStats::default();

    // Create screenshots directory if it doesn't exist
    std::fs::create_dir_all("screenshots").unwrap_or_else(|_| {
        println!("Could not create screenshots directory");
    });

    loop {
        if last_update.elapsed() >= Duration::from_secs(interval) {
            let commands = vec![
                "top -bn1 | head -n 20", // Get more lines from top to ensure we capture CPU info
                "free -b",
                "df -B1",
                "uptime",
            ];

            let mut output = String::new();
            for cmd in &commands {
                let mut channel = sess.channel_session()?;
                channel.exec(cmd)?;
                let mut cmd_output = String::new();
                channel.read_to_string(&mut cmd_output)?;
                output.push_str(&cmd_output);
                channel.wait_close()?;
            }

            // Save the existing CPU history
            let existing_history = stats.cpu_history.clone();
            
            // Get the new stats
            stats = parse_system_stats(&output);
            
            // Restore the existing history and then add the new data point
            stats.cpu_history = existing_history;
            stats.update_cpu_history();
            
            last_update = Instant::now();
        }

        terminal.draw(|f| {
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),  // System info
                    Constraint::Length(10), // CPU history graph
                    Constraint::Length(3),  // Memory bars
                    Constraint::Length(4),  // Further reduced disk usage section from 6 to 4
                ].as_ref())
                .split(size);

            // System info (uptime + load)
            let uptime_text = Text::from(vec![
                Line::from(vec![
                    Span::raw(stats.uptime.clone()),
                    Span::raw(" "),
                    Span::styled("(Press 'q' to quit)", Style::default().fg(Color::Gray)),
                ]),
            ]);
            let uptime_widget = Paragraph::new(uptime_text)
                .block(Block::default().borders(Borders::ALL).title("System"));
            f.render_widget(uptime_widget, chunks[0]);

            // CPU history
            let width = chunks[1].width as f64;
            // Ensure we have at least two points
            if stats.cpu_history.is_empty() {
                stats.cpu_history.push(stats.cpu_usage);
                stats.cpu_history.push(stats.cpu_usage);
            }
            
            let cpu_points: Vec<(f64, f64)> = stats.cpu_history.iter().enumerate()
                .map(|(i, &v)| {
                    let x = if stats.cpu_history.len() > 1 {
                        (i as f64 / (stats.cpu_history.len() - 1) as f64) * width
                    } else {
                        0.0
                    };
                    (x, v)
                })
                .collect();

            let datasets = vec![
                Dataset::default()
                    .name("CPU %")
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::Cyan))
                    .data(&cpu_points)
            ];

            let cpu_chart = Chart::new(datasets)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .title(format!("CPU Usage: {:.1}%", stats.cpu_usage)))
                .x_axis(Axis::default()
                    .style(Style::default().fg(Color::Gray))
                    .bounds([0.0, width]))
                .y_axis(Axis::default()
                    .style(Style::default().fg(Color::Gray))
                    .bounds([0.0, 100.0]));
            f.render_widget(cpu_chart, chunks[1]);

            // Memory usage
            let mem_percent = (stats.memory_used as f64 / stats.memory_total as f64 * 100.0) as u64;
            let swap_percent = (stats.swap_used as f64 / stats.swap_total as f64 * 100.0) as u64;
            let memory_data = [("Memory", mem_percent), ("Swap", swap_percent)];

            let barchart = BarChart::default()
                .block(Block::default().borders(Borders::ALL).title("Memory"))
                .data(&memory_data[..])
                .bar_width(10)
                .group_gap(3)
                .max(100);
            f.render_widget(barchart, chunks[2]);

            // Disk usage
            let disk_items: Vec<ListItem> = stats.disk_usage
                .iter()
                .map(|(mount, total, used)| {
                    let percentage = (*used as f64 / *total as f64 * 100.0) as u8;
                    let text = format!(
                        "{}: {} / {} ({}%)",
                        mount,
                        format_size(*used, BINARY),
                        format_size(*total, BINARY),
                        percentage
                    );
                    ListItem::new(text)
                })
                .collect();
            let disk_list = List::new(disk_items)
                .block(Block::default().borders(Borders::ALL).title("Disk Usage"));
            f.render_widget(disk_list, chunks[3]);
        })?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('s') => {
                        // Take a screenshot (macOS specific)
                        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
                        let filename = format!("screenshots/remote_management_{}.png", timestamp);
                        
                        // Temporarily restore the terminal to normal mode
                        disable_raw_mode()?;
                        std::io::stdout().execute(LeaveAlternateScreen)?;
                        
                        // Short delay to ensure screen is visible
                        std::thread::sleep(Duration::from_millis(500));
                        
                        // Take screenshot
                        let status = std::process::Command::new("screencapture")
                            .arg("-x") // Capture without sound
                            .arg(filename.clone())
                            .status();
                        
                        // Return to alternate screen mode
                        std::io::stdout().execute(EnterAlternateScreen)?;
                        enable_raw_mode()?;
                        
                        if let Ok(status) = status {
                            if status.success() {
                                // Show a notification on the screen that screenshot was taken
                                terminal.draw(|f| {
                                    let size = f.size();
                                    let message = format!("Screenshot saved to {}", filename);
                                    
                                    // Use fixed dimensions for the popup
                                    let width = message.len() as u16 + 4; // Add some padding
                                    let height = 3; // 1 for text, 2 for borders
                                    
                                    let popup_area = Rect {
                                        x: (size.width - width) / 2,
                                        y: (size.height - height) / 2,
                                        width,
                                        height,
                                    };
                                    
                                    let notification = Paragraph::new(message)
                                        .style(Style::default().fg(Color::Green))
                                        .block(Block::default().borders(Borders::ALL));
                                    
                                    f.render_widget(notification, popup_area);
                                })?;
                                
                                // Wait for 2 seconds to show the notification
                                std::thread::sleep(Duration::from_secs(2));
                            }
                        }
                    },
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    std::io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    match args.command {
        Commands::Status { host, username, port } => {
            match get_server_status(&host, port, username) {
                Ok(status) => println!("{}", status),
                Err(e) => eprintln!("Error: {:#}", e),
            }
        }
        Commands::Monitor { host, username, port, interval } => {
            let address = format!("{}:{}", host, port);
            let tcp = TcpStream::connect(&address)
                .with_context(|| format!("Failed to connect to {}", address))?;
            
            let mut sess = Session::new()?;
            sess.set_tcp_stream(tcp);
            sess.handshake()?;

            // Try SSH agent first
            if let Some(user) = &username {
                if sess.userauth_agent(user).is_ok() {
                    monitor_system(&mut sess, interval).await?;
                    return Ok(());
                }
            }

            // If SSH agent fails or no username provided, prompt for credentials
            let (username, password) = get_credentials(username)?;
            sess.userauth_password(&username, &password)
                .with_context(|| "Authentication failed")?;

            monitor_system(&mut sess, interval).await?;
        }
    }

    Ok(())
}

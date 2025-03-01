# 🖥️ Remote Management CLI

![License](https://img.shields.io/badge/license-MIT-blue)
![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)

A powerful terminal-based remote server monitoring tool written in Rust. Monitor your remote servers' performance metrics in real-time with beautiful TUI graphs and visualizations.

## ✨ Features

- **Real-time monitoring** of CPU, memory, and disk usage
- **Historical CPU graphs** to visualize performance over time
- **Clean, responsive terminal UI** built with Ratatui
- **Secure SSH connections** with password or SSH agent authentication
- **Low overhead** monitoring with minimal impact on server resources

## 📊 Screenshots

```

```

## 🚀 Installation

### Prerequisites

- Rust 1.70 or higher
- OpenSSL development libraries

### Building from source

```bash
# Clone the repository
git clone https://github.com/yourusername/remote-management.git
cd remote-management

# Build with Cargo
cargo build --release

# Run the binary
./target/release/remote_management --help
```

## 📖 Usage

The tool provides two main commands:

### Status

Get a quick summary of a remote server's status:

```bash
remote_management status -H server.example.com -u username
```

### Monitor

Start real-time monitoring of a remote server:

```bash
remote_management monitor -H server.example.com -u username -i 2
```

#### Command-line options

- `-H, --host`: Remote host address (required)
- `-u, --username`: SSH username (optional, will prompt if not provided)
- `-P, --port`: SSH port (default: 22)
- `-i, --interval`: Update interval in seconds (default: 1)

## ⌨️ Keyboard shortcuts

While monitoring:
- `q`: Quit the application

## 🔧 Authentication

The application supports:
1. SSH agent authentication (tried first)
2. Password authentication (fallback)

## 🔒 Security

- No credentials are stored by the application
- All connections are secured via SSH
- Minimal server access requirements (only needs to run basic system commands)

## 📜 License

This project is licensed under the MIT License - see the LICENSE file for details.

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

---

Made with ❤️ and Rust
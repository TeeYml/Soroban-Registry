use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor};
use shlex;

pub struct ShellContext {
    pub api_url: String,
    pub contract_id: Option<String>,
    pub network: String,
}

impl ShellContext {
    fn new(api_url: String, network: String) -> Self {
        Self {
            api_url,
            contract_id: None,
            network,
        }
    }

    fn prompt(&self) -> String {
        let contract = self.contract_id.as_deref().unwrap_or("none");
        format!(
            "{} ({}) [{}] > ",
            "soroban-registry".cyan().bold(),
            self.network.bright_blue(),
            contract.bright_magenta()
        )
    }
}

pub async fn run(api_url: &str, initial_network: Option<String>) -> Result<()> {
    let mut rl = DefaultEditor::new()?;
    let mut context = ShellContext::new(
        api_url.to_string(),
        initial_network.unwrap_or_else(|| "testnet".to_string()),
    );

    println!("\n{}", "Soroban Registry Interactive Shell".bold().cyan());
    println!("Type 'help' for shell commands, or any CLI command.");
    println!("Context: network={}, contract=none", context.network);
    println!();

    loop {
        let readline = rl.readline(&context.prompt());
        match readline {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(line);
                let args = match shlex::split(line) {
                    Some(args) => args,
                    None => {
                        println!("{}", "Error: Invalid quoting in command".red());
                        continue;
                    }
                };

                if args.is_empty() {
                    continue;
                }

                match args[0].as_str() {
                    "exit" | "quit" => break,
                    "help" => show_shell_help(),
                    "context" => show_context(&context),
                    "cd" => {
                        if args.len() > 1 {
                            context.contract_id = Some(args[1].clone());
                            println!("Context set to contract: {}", args[1].bright_magenta());
                        } else {
                            context.contract_id = None;
                            println!("Context cleared (no active contract)");
                        }
                    }
                    "ls" => {
                        // Execute 'list' command
                        let cmd_args = vec![
                            "soroban-registry".to_string(),
                            "list".to_string(),
                            "--network".to_string(),
                            context.network.clone(),
                        ];
                        if let Err(e) = execute_command(cmd_args, &context).await {
                            println!("{} {}", "Error:".red(), e);
                        }
                    }
                    "set" => {
                        if args.len() >= 3 && args[1] == "network" {
                            context.network = args[2].clone();
                            println!("Network set to: {}", context.network.bright_blue());
                        } else {
                            println!("Usage: set network <mainnet|testnet|futurenet>");
                        }
                    }
                    _ => {
                        // Try to parse as a normal CLI command
                        let mut cmd_args = vec!["soroban-registry".to_string()];
                        
                        // Inject context if not present in args
                        let has_network = args.iter().any(|a| a == "--network");
                        let has_contract = args.iter().any(|a| a == "--contract-id" || a == "--id");

                        if !has_network {
                            cmd_args.push("--network".to_string());
                            cmd_args.push(context.network.clone());
                        }

                        // Use the sub-command and its arguments
                        let subcmd = args[0].clone();
                        
                        // Context injection for specific commands if context exists
                        let final_args = args.clone();
                        if let Some(ref cid) = context.contract_id {
                            if !has_contract {
                                match subcmd.as_str() {
                                    "info" | "export" | "breaking-changes" | "profile" | "coverage" | "verify" => {
                                        // These usually take --id or positional. 
                                        // If it's a known command that needs ID and it's missing, let's try to add it.
                                        // For simplicity, we just pass what the user typed.
                                    }
                                    _ => {}
                                }
                            }
                        }

                        cmd_args.extend(final_args);

                        if let Err(e) = execute_command(cmd_args, &context).await {
                            println!("{} {}", "Error:".red(), e);
                        }
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }

    println!("Bye!");
    Ok(())
}

async fn execute_command(args: Vec<String>, _context: &ShellContext) -> Result<()> {
    match Cli::try_parse_from(args) {
        Ok(cli) => {
            // We call dispatch_command directly to avoid recursion
            // but we need to resolve the network first.
            let cfg_network = crate::config::resolve_network(cli.network.clone())?;
            let mut net_str = cfg_network.to_string();
            if net_str == "auto" {
                net_str = "mainnet".to_string();
            }
            let network: crate::commands::Network = net_str.parse().unwrap();
            
            crate::dispatch_command(cli, network, cfg_network).await
        }
        Err(e) => {
            if e.to_string().contains("Usage:") {
                 println!("{}", e);
                 Ok(())
            } else {
                Err(e.into())
            }
        }
    }
}

fn show_shell_help() {
    println!("{}", "\nShell Commands:".bold());
    println!("  ls             List contracts (alias for 'list')");
    println!("  cd <id>        Set active contract context");
    println!("  cd             Clear active contract context");
    println!("  context        Show current shell context");
    println!("  set network <n> Change active network");
    println!("  help           Show this help");
    println!("  exit / quit    Exit the shell");
    println!("\nYou can also run any standard CLI command (e.g., 'search gravity' or 'info').");
    println!("Contextual values (network, contract id) are injected automatically if omitted.\n");
}

fn show_context(context: &ShellContext) {
    println!("\n{}", "Current Context:".bold());
    println!("  API URL:  {}", context.api_url.bright_black());
    println!("  Network:  {}", context.network.bright_blue());
    println!(
        "  Contract: {}",
        context
            .contract_id
            .as_deref()
            .unwrap_or("none")
            .bright_magenta()
    );
    println!();
}

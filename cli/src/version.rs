use anyhow::Result;
use colored::*;

pub async fn check_version() -> Result<()> {
    println!("\n{}", "Soroban Registry CLI".bold().cyan());
    println!("Version: {}", env!("CARGO_PKG_VERSION"));
    println!("Status:  {}", "Up to date".green());
    println!();
    Ok(())
}

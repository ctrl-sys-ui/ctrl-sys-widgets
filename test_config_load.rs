// Quick test to verify app config loading with improved error messages
// Run with: cargo run --bin test-config-load examples/demo_app.json

use mycela::config::AppConfig;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    
    let config_path = if args.len() > 1 {
        &args[1]
    } else {
        "examples/demo_app.json"
    };
    
    println!("Attempting to load config from: {}\n", config_path);
    
    match AppConfig::load(config_path) {
        Ok(config) => {
            println!(" SUCCESS! Loaded configuration:");
            println!("   Title: {}", config.title);
            println!("   Home screen: {}", config.home_screen.as_deref().unwrap_or("(none)"));
            println!("   Screens: {}", config.screens.len());
            for screen in &config.screens {
                println!(
                    "      - {}: {} ({} widgets)",
                    screen.id,
                    screen.title,
                    screen.widgets.len()
                );
            }
        }
        Err(e) => {
            eprintln!("❌ FAILED to load configuration:\n");
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

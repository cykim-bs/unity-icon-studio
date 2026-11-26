//! `uis`: the command line of Icon Studio. It uses the same home folder and functions as the app.
//! Commands print JSON with `--json` (plain text otherwise).

use serde_json::json;
use uis_core::home::Home;

const USAGE: &str = "usage: uis <command> [options]

commands:
  home [--json]        the tool's home folder and settings file
  settings             the settings in effect (JSON)
  --version            the version
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json_out = args.iter().any(|a| a == "--json");
    let rest: Vec<&str> = args.iter().map(String::as_str).filter(|a| *a != "--json").collect();
    let code = match rest.first().copied() {
        Some("--version") | Some("-V") => {
            println!("uis {}", uis_core::VERSION);
            0
        }
        Some("home") => {
            let home = Home::resolve();
            let value = json!({
                "home": home.root(),
                "override": Home::is_override(),
                "settingsFile": home.settings_file(),
            });
            print_value(&value, json_out);
            0
        }
        Some("settings") => {
            let home = Home::resolve();
            let settings = uis_core::settings::load(&home);
            print_value(&serde_json::to_value(&settings).unwrap_or_default(), true);
            0
        }
        Some("help") | Some("--help") | Some("-h") | None => {
            print!("{USAGE}");
            0
        }
        Some(other) => {
            eprintln!("uis: unknown command {other:?}\n\n{USAGE}");
            2
        }
    };
    std::process::exit(code);
}

pub(crate) fn print_value(value: &serde_json::Value, json_out: bool) {
    if json_out {
        println!("{}", serde_json::to_string_pretty(value).unwrap_or_default());
    } else if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            println!("{k}: {}", v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()));
        }
    }
}

use colored::*;

/// Print success message with green checkmark
pub fn print_success(msg: &str) {
    println!("{} {}", "✓".green().bold(), msg.green());
}

/// Print error message with red cross
pub fn print_error(msg: &str) {
    eprintln!("{} {}", "✗".red().bold(), msg.red());
}

/// Print warning message with yellow warning sign
pub fn print_warning(msg: &str) {
    println!("{} {}", "⚠".yellow().bold(), msg.yellow());
}

/// Print info message with blue info sign
pub fn print_info(msg: &str) {
    println!("{} {}", "ℹ".blue().bold(), msg.blue());
}

/// Print header with bold and underline
pub fn print_header(msg: &str) {
    println!("{}", msg.bold().underline());
}

/// Print a section title
pub fn print_section(msg: &str) {
    println!("{}", msg.bold());
}

/// Print a file path with appropriate formatting
pub fn print_path(path: &str) {
    println!("  {}", path.cyan());
}

/// Print a list item
pub fn print_list_item(index: usize, item: &str) {
    println!("  {}. {}", index, item);
}

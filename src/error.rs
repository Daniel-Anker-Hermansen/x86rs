use std::process::exit;

pub fn fatal(message: &str) -> ! {
	eprintln!("Fatal error: {message}");
	exit(0)
}

pub fn info(message: &str) {
	eprintln!("Info: {message}");
}

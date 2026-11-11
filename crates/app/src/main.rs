pub mod shared;

fn main() -> Result<(), &'static str> {
    let _library = shared::load_cef();

    let args = cef::args::Args::new();
    let Some(cmd_line) = args.as_cmd_line() else {
        return Err("Failed to parse command line arguments");
    };

    shared::run_main(args.as_main_args(), &cmd_line, std::ptr::null_mut());
    Ok(())
}

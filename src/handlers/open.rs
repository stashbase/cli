pub fn handle_open_dashboard() {
    // TODO: get workspace id
    let url = "http://localhost:3000/workspace/4ef8a291-024e-4ed8-924b-1cc90d01315e/projects";

    eprint!("Opening URL: {}", url);

    if let Err(err) = webbrowser::open(&url) {
        eprintln!("{}", &format!("Error opening URL: {}", err));
    }
}

#[allow(unused_imports)]
use std::env;
#[allow(unused_imports)]
use std::fs;

use git_starter_rust::App;

fn main() {
    // Uncomment this block to pass the first stage
    let args: Vec<String> = env::args().collect();

    App::new().run(args); 

}

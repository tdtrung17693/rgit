#[allow(unused_imports)]
use std::env;
#[allow(unused_imports)]
use std::fs;

mod core;

fn main() {
    // Uncomment this block to pass the first stage
    let args: Vec<String> = env::args().collect();

    core::App::new().run(args); 

}

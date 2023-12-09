use std::{fs, io::Read};

pub struct App {}

impl App {
    pub fn new() -> Self {
        Self {}
    }

    pub fn run(&self, args: Vec<String>) {
        if args[1] == "init" {
            self.init()
        } else if args[1] == "cat-file" {
            let blob_sha = args[3].clone();
            self.cat_file(blob_sha)
        }
    }

    fn init(&self) {
        fs::create_dir(".git").unwrap();
        fs::create_dir(".git/objects").unwrap();
        fs::create_dir(".git/refs").unwrap();
        fs::write(".git/HEAD", "ref: refs/heads/master\n").unwrap();
        println!("Initialized git directory")
    }

    fn cat_file(&self, blob_sha: String) {
        let subfolder = &blob_sha[0..2];
        let binary_content = fs::read(format!(".git/objects/{}/{}", subfolder, &blob_sha[2..])).unwrap();
        let mut content = String::new();
        flate2::read::ZlibDecoder::new(&binary_content[..]).read_to_string(&mut content).unwrap();
        let (_, content) = content.split_once('\x00').unwrap();
        print!("{}", content);
    }
}

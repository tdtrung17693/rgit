use hex_literal::hex;
use sha1::{Digest, Sha1};
use std::{
    fs,
    io::{Read, Write},
};

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
        } else if args[1] == "hash-object" {
            let file_path = args[3].clone();
            self.hash_object(&file_path)
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
        let binary_content =
            fs::read(format!(".git/objects/{}/{}", subfolder, &blob_sha[2..])).unwrap();
        let mut content = String::new();
        flate2::read::ZlibDecoder::new(&binary_content[..])
            .read_to_string(&mut content)
            .unwrap();
        let (_, content) = content.split_once('\x00').unwrap();
        print!("{}", content);
    }

    fn hash_object(&self, file_path: &str) {
        let content = fs::read(file_path).unwrap();
        let header_bytes = format!("blob {}\0", content.len()).into_bytes();
        let content = [&header_bytes[..], &content[..]].concat();
        let mut compressed = Vec::new();
        let mut compressor =
            flate2::write::ZlibEncoder::new(&mut compressed, flate2::Compression::fast());
        compressor.write_all(&content).unwrap();
        compressor.finish().unwrap();
        let mut hasher = sha1::Sha1::new();
        hasher.update(&content);
        let hash = hasher.finalize();

        println!("{}", hex::encode(hash));
        let hash = hex::encode(hash);
        let subfolder = &hash[0..2];
        fs::create_dir_all(format!(".git/objects/{}/", subfolder)).unwrap();
        match fs::write(
            format!(".git/objects/{}/{}", subfolder, &hash[2..]),
            compressed,
        ) {
            Ok(_) => {}
            Err(e) => println!("{e}")
        }
    }
}

use sha1::Digest;
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
        } else if args[1] == "ls-tree" {
            let tree_sha = args[3].clone();
            self.ls_tree(tree_sha)
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
        let mut content = Vec::new();
        flate2::read::ZlibDecoder::new(&binary_content[..])
            .read_to_end(&mut content)
            .unwrap();
        let content = String::from_utf8_lossy(&content);
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
            Err(e) => println!("{e}"),
        }
    }

    fn ls_tree(&self, tree_sha: String) {
        let content = self.read_object(&tree_sha);
        let str_content = String::from_utf8_lossy(&content[..]);
        let normalized_tree_sha =
            if content.starts_with(b"tree") && str_content.split_once('\x00').is_none() {
                let (_, content) = content.split_at(5);

                hex::encode(&content[0..20])
            } else {
                tree_sha
            };

        let content = self.read_object(&normalized_tree_sha);

        let parts = content.split(|x| *x == b'\0');

        let mut entries = parts
            .skip(1)
            .enumerate()
            .filter(|(_, x)| !x.is_empty())
            .fold(Vec::new(), |mut acc, (i, part)| {
                if i == 0 {
                    let (mode, name) = std::str::from_utf8(part).unwrap().split_once(' ').unwrap();
                    acc.push((mode, name, "".into()));
                } else {
                    let sha = &part[0..20];
                    acc[i - 1].2 = hex::encode(sha);

                    if let Some((mode, name)) =
                        std::str::from_utf8(&part[20..]).unwrap().split_once(' ')
                    {
                        acc.push((mode, name, "".into()))
                    };
                }

                acc
            });
        entries.sort_by(|a, b| a.1.cmp(b.1));
        for (_mode, name, _sha) in entries {
            println!("{}", name)
        }
    }

    fn read_object(&self, blob_sha: &str) -> Vec<u8> {
        let subfolder = &blob_sha[0..2];
        let binary_content =
            fs::read(format!(".git/objects/{}/{}", subfolder, &blob_sha[2..])).unwrap();
        let mut content = Vec::new();
        flate2::read::ZlibDecoder::new(&binary_content[..])
            .read_to_end(&mut content)
            .unwrap();

        content
    }
}

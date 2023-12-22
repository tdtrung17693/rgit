use sha1::Digest;
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
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
            self.hash_object(&file_path);
        } else if args[1] == "ls-tree" {
            let tree_sha = args[3].clone();
            self.ls_tree(tree_sha)
        } else if args[1] == "write-tree" {
            self.write_tree()
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

    fn hash_object(&self, file_path: &str) -> Vec<u8> {
        let (compressed, bin_hash) = self.make_blob_object(file_path);
        let hash = hex::encode(&bin_hash[..]);
        let subfolder = &hash[0..2];
        fs::create_dir_all(format!(".git/objects/{}/", subfolder)).unwrap();
        match fs::write(
            format!(".git/objects/{}/{}", subfolder, &hash[2..]),
            compressed,
        ) {
            Ok(_) => {}
            Err(e) => println!("{e}"),
        }

        println!("{}", hash);
        bin_hash
    }

    fn make_blob_object(&self, file_path: &str) -> (Vec<u8>, Vec<u8>) {
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
        (compressed, hash.as_slice().to_vec())
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

    fn write_tree(&self) {
        let tree_hash = self.make_tree_object(".");
        println!("{}", hex::encode(&tree_hash));
    }

    fn make_tree_object(&self, path: &str) -> Vec<u8> {
        let mut tree_entries = Vec::new();
        let git_ignore = if let Ok(file) = fs::read(".gitignore") {
            String::from_utf8(fs::read(".gitignore").unwrap()).unwrap()
        } else {
            "".into()
        };

        if let Ok(entries) = fs::read_dir(path) {
            entries.filter(|entry| {
                let file_name = entry.as_ref().unwrap().file_name();
                if git_ignore.contains(file_name.to_str().unwrap()) {
                    return false;
                }

                if file_name.to_str().unwrap() == ".git" {
                    return false;
                }

                return true;
            }).for_each(|entry| {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    let mode = format!("{:o}", 0o40000);
                    let tree_hash = self.make_tree_object(entry.path().to_str().unwrap());
                    tree_entries.push((
                        mode,
                        entry.file_name().to_string_lossy().to_string(),
                        tree_hash,
                    ));
                } else if entry.file_type().unwrap().is_file() {
                    let perms = entry.metadata().unwrap().permissions();
                    let mode = format!("{:o}", perms.mode());
                    let (_, hash) = self.make_blob_object(entry.path().to_str().unwrap());
                    tree_entries.push((
                        mode,
                        entry.file_name().to_string_lossy().to_string(),
                        hash,
                    ));
                }
            })
        }

        tree_entries.sort_by(|a, b| a.1.cmp(&b.1));
        let mut content: Vec<u8> = vec![];

        /**
         * [mode] [file/folder name]\0[SHA-1 of referencing blob or tree]
         * **/
        for (mode, name, sha) in tree_entries {
            content.extend(format!("{} {}\0", mode, name).as_bytes());
            content.extend(sha);
        }
        let header = format!("tree {}\0", content.len()).into_bytes();
        let content = [&header[..], &content[..]].concat();

        let mut compressed = Vec::new();
        let mut compressor =
            flate2::write::ZlibEncoder::new(&mut compressed, flate2::Compression::fast());
        compressor.write_all(&content).unwrap();
        compressor.finish().unwrap();

        let mut hasher = sha1::Sha1::new();
        hasher.update(&content);
        let bin_hash = hasher.finalize();

        let hash = hex::encode(&bin_hash[..]);
        let subfolder = &hash[0..2];
        fs::create_dir_all(format!(".git/objects/{}/", subfolder)).unwrap();
        match fs::write(
            format!(".git/objects/{}/{}", subfolder, &hash[2..]),
            compressed,
        ) {
            Ok(_) => {}
            Err(e) => println!("{e}"),
        }

        bin_hash.as_slice().to_vec()
    }
}

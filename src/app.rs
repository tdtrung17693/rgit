use chrono::{NaiveDate, TimeZone, Utc};
use sha1::Digest;
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
};

use crate::git_client::{get_refs, get_objects, Repo};


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
        } else if args[1] == "commit-tree" {
            let tree_sha = args[2].clone();
            let message = if args.len() == 5 {
                args[4].clone()
            } else {
                args[6].clone()
            };
            let parent_hash = if args.len() == 7 {
                Some(&args[4])
            } else {
                None
            };
            self.commit_tree(&tree_sha, &message, parent_hash.map(|x| &x[..]));
        } else if args[1] == "clone" {
            let url = args[2].strip_suffix('/').clone().unwrap_or(&args[2]);
            let dir = if args.len() == 4 {
                &args[3]
            } else {
                ""
            };
            self.clone(&url, dir);
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
        self.persist_git_object(&bin_hash[..], &compressed[..]);

        bin_hash
    }

    fn make_blob_object(&self, file_path: &str) -> (Vec<u8>, Vec<u8>) {
        let content = fs::read(file_path).unwrap();
        self.make_git_object(&content, "blob")
    }

    fn make_git_object(&self, content: &[u8], obj_type: &str) -> (Vec<u8>, Vec<u8>) {
        let header_bytes = format!("{obj_type} {}\0", content.len()).into_bytes();
        let content = [&header_bytes[..], content].concat();
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

    fn persist_git_object(&self, bin_hash: &[u8], compressed_content: &[u8]) {
        let hash = hex::encode(bin_hash);
        let subfolder = &hash[0..2];
        fs::create_dir_all(format!(".git/objects/{}/", subfolder)).unwrap();
        match fs::write(
            format!(".git/objects/{}/{}", subfolder, &hash[2..]),
            compressed_content,
        ) {
            Ok(_) => {}
            Err(e) => println!("{e}"),
        }
        println!("{}", hash);
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
                    if part.len() < 20 {
                        return acc
                    }
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

    fn parse_tree_object(&self, content: &[u8]) {
        let parts = content.split(|x| *x == b'\n');

        let mut entries = parts
            .skip(1)
            .enumerate()
            .filter(|(_, x)| !x.is_empty())
            .fold(Vec::new(), |mut acc, (i, part)| {
                if i == 0 {
                    let (mode, name) = std::str::from_utf8(part).unwrap().split_once(' ').unwrap();
                    acc.push((mode, name, "".into()));
                } else {
                    if part.len() < 20 {
                        panic!("Invalid tree entry: {}", String::from_utf8_lossy(part));
                    }
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
        println!("{}", hex::encode(tree_hash));
    }

    fn make_tree_object(&self, path: &str) -> Vec<u8> {
        let mut tree_entries = Vec::new();
        let git_ignore = if let Ok(file) = fs::read(".gitignore") {
            String::from_utf8(file).unwrap_or_default()
        } else {
            "".into()
        };

        if let Ok(entries) = fs::read_dir(path) {
            entries
                .filter(|entry| {
                    let file_name = entry.as_ref().unwrap().file_name();
                    if git_ignore.contains(file_name.to_str().unwrap()) {
                        return false;
                    }

                    if file_name.to_str().unwrap() == ".git" {
                        return false;
                    }

                    true
                })
                .for_each(|entry| {
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
        let content: Vec<u8> =
            tree_entries
                .iter()
                .fold(vec![], |mut content, (mode, name, sha)| {
                    content.extend(format!("{} {}\0", mode, name).as_bytes());
                    content.extend(sha);
                    content
                });

        /*
        [mode] [file/folder name]\0[SHA-1 of referencing blob or tree]
        */
        let (compressed, bin_hash) = self.make_git_object(&content, "tree");
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

    fn commit_tree(&self, tree_hash: &str, message: &str, parent_hash: Option<&str>) {
        let bin_hash = self.make_commit_object(tree_hash, message, parent_hash);
        let hash = hex::encode(&bin_hash[..]);
        println!("{}", hash);
    }

    fn make_commit_object(
        &self,
        tree_hash: &str,
        message: &str,
        parent_hash: Option<&str>,
    ) -> Vec<u8> {
        let committer_name = "Trung Tran";
        let committer_email = "trungtran@email.com";
        let mut content: Vec<u8> = Vec::new();
        let now = chrono::Local::now();
        let timestamp = now.timestamp();
        // let timezone = now.timezone().offset_from_local_date();
        let offset = now.offset();
        let hour = offset.local_minus_utc() / 3600;
        let timezone = format!("{}{:02}00", if hour < 0 { "-" } else { "+" }, hour.abs());

        content.extend(format!("tree {}\n", tree_hash).as_bytes());
        if let Some(parent_hash) = parent_hash {
            content.extend(format!("parent {}\n", parent_hash).as_bytes());
        }
        content.extend(
            format!(
                "author {} <{}> {} {}\n",
                committer_name, committer_email, timestamp, timezone
            )
            .as_bytes(),
        );
        content.extend(
            format!(
                "committer {} <{}> {} {}\n\n",
                committer_name, committer_email, timestamp, timezone
            )
            .as_bytes(),
        );
        content.extend(format!("{}\n", message).as_bytes());
        let (compressed, bin_hash) = self.make_git_object(&content, "commit");
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

    fn clone(&self, url: &str, path: &str) {
        // let refs 
        // let refs = get_refs(url).unwrap();
        // let packs = get_objects(url, refs.refs.iter().map(|x| x.hash.clone()).collect());
        let current_dir = std::env::current_dir().unwrap();
        let path = format!("{}/{}", current_dir.to_str().unwrap(), path);
        println!("path = {path}");
        println!("url = {url}");
        let mut repo = Repo::new(url, &path);
        repo.clone();
    }

    fn timestamp() -> u128 {
        let time = std::time::SystemTime::now();
        let since_the_epoch = time.duration_since(std::time::UNIX_EPOCH).unwrap();
        since_the_epoch.as_millis()
    }
}

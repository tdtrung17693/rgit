use std::{
    collections::HashMap,
    fs,
    io::{self, BufRead, BufReader, Read, Seek, Write},
    path,
};

use sha1::Digest;

use reqwest::blocking as reqwest;

#[derive(Debug)]
pub struct Ref {
    pub name: String,
    pub hash: String,
}

#[derive(Debug)]
pub struct Refs {
    pub refs: HashMap<String, String>,
    services: Vec<String>,
    head: String,
}

#[derive(Clone)]
pub enum GitObjectType {
    Blob,
    Commit,
    Tag,
    Tree,
}

impl From<u8> for GitObjectType {
    fn from(obj_type: u8) -> Self {
        match obj_type {
            1 => GitObjectType::Commit,
            2 => GitObjectType::Tree,
            3 => GitObjectType::Blob,
            4 => GitObjectType::Tag,
            _ => panic!("unknown object type: {}", obj_type),
        }
    }
}

impl std::fmt::Display for GitObjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitObjectType::Blob => write!(f, "blob"),
            GitObjectType::Commit => write!(f, "commit"),
            GitObjectType::Tag => write!(f, "tag"),
            GitObjectType::Tree => write!(f, "tree"),
        }
    }
}

pub struct GitObject {
    id: String,
    content: Vec<u8>,
    size: u128,
    object_type: GitObjectType,
}

impl GitObject {
    fn new(content: Vec<u8>, object_type: GitObjectType) -> GitObject {
        let size = content.len() as u128;
        let header = format!("{} {}\0", object_type, size).into_bytes();
        // println!("header: {:?}", String::from_utf8_lossy(&header[..]));

        let content_with_header = [&header[..], &content[..]].concat();
        let mut hasher = sha1::Sha1::new();
        hasher.update(&content_with_header);
        let hash = hasher.finalize();
        GitObject {
            id: hex::encode(hash.as_slice()),
            content,
            size,
            object_type,
        }
    }

    fn persist(&self, object_dir: &str) {
        let id = &self.id;
        let subfolder = &id[0..2];
        let filename = &id[2..];
        let path = format!("{}/{}", object_dir, subfolder);
        if fs::create_dir_all(&path).is_err() {
            panic!("Failed to create .git/objects/{} directory", subfolder);
        }
        let header = format!("{} {}\0", self.object_type, self.size).into_bytes();
        let content = [&header[..], &self.content[..]].concat();
        let mut compressed = Vec::new();
        let mut compressor =
            flate2::write::ZlibEncoder::new(&mut compressed, flate2::Compression::fast());
        compressor.write_all(&content).unwrap();
        compressor.finish().unwrap();
        fs::write(format!("{}/{}", path, filename), compressed).unwrap();
    }
}

pub fn get_refs(git_url: &str) -> Result<Refs, Box<dyn std::error::Error>> {
    let body = reqwest::get(format!("{}/info/refs?service=git-upload-pack", git_url).as_str())?;
    let body = body.bytes().unwrap();
    let body = String::from_utf8_lossy(&body[..]);
    let parts = body.split('\n').skip(1);
    let mut services = Vec::new();
    let mut head = String::new();
    let refs = parts
        .filter(|part| *part != "0000")
        .map(|part| {
            let parts: Vec<&str> = part.split('\0').collect();
            let mut ref_name = String::new();
            let mut ref_hash = String::new();
            if parts.len() == 2 {
                // println!("{}", parts[0]);
                let (header, current_ref_name) = parts[0][4..].split_once(' ').unwrap();
                ref_hash = header[4..].to_string();
                ref_name = current_ref_name.to_string();
                head = ref_hash.clone();
                services = parts[1].split(' ').map(|x| x.to_string()).collect();
            } else {
                let (head, current_ref_name) = parts[0].split_once(' ').unwrap();
                ref_hash = head[4..].to_string();
                ref_name = current_ref_name.to_string();
            }

            (ref_name, ref_hash)
        })
        .collect();

    Ok(Refs {
        refs,
        services,
        head,
    })
}

pub struct Repo {
    objects: HashMap<String, GitObject>,
    head: String,
    remote: String,
    git_dir: String,
    refs: HashMap<String, String>,
}

impl Repo {
    pub fn new(remote: &str, git_dir: &str) -> Repo {
        Repo {
            objects: HashMap::new(),
            head: String::new(),
            remote: remote.to_string(),
            git_dir: git_dir.to_string(),
            refs: HashMap::new(),
        }
    }

    pub fn clone(&mut self) {
        let refs = get_refs(&self.remote).unwrap();
        self.refs = refs.refs;
        let hashes = self.refs.values().cloned().collect();
        self.objects = get_objects(&self.remote, hashes).unwrap();
        // println!("{:#?}", self.refs);
        self.persist_objects();
        self.populate_refs();
        self.checkout_head();
    }

    fn checkout_head(&mut self) {
        let commit_object = &self.objects[&self.head];
        let tree_object = String::from_utf8(commit_object.content.clone()).unwrap();
        let (tree_line, _) = tree_object.split_once('\n').unwrap();
        let tree = tree_line.replace("tree ", "");
        let mut pool = vec![(self.git_dir.clone(), tree)];
        while !pool.is_empty() {
            let (path, tree_id) = pool.pop().unwrap();
            // println!("treeid = {tree_id} - {path}");
            let entries = parse_tree_object(&self.objects[&tree_id].content);
            for (mode, name, sha) in entries {
                let path = format!("{path}/{name}");
                // println!("mode: {mode} - name: {name} - sha: {sha}");
                if mode == "40000" {
                    fs::create_dir_all(&path);
                    pool.push((path, sha));
                } else {
                    let blob_object = &self.objects[&sha];
                    // println!("blob {sha}: {path}");
                    fs::write(path, &blob_object.content);
                }
            }
        }
    }

    fn populate_refs(&mut self) {
        let refs_dir = format!("{}/.git/refs", self.git_dir);

        if fs::create_dir_all(refs_dir).is_err() {
            panic!("Failed to create .git/refs directory");
        }

        self.head = self
            .refs
            .iter()
            .find(|(ref_name, _)| *ref_name == "HEAD")
            .unwrap()
            .1
            .clone();

        self.refs.iter().for_each(|(ref_name, ref_hash)| {
            if ref_name == "HEAD"
                || ref_name.starts_with("refs/pull")
                || ref_name.starts_with("refs/tags")
            {
                return;
            }

            if ref_hash == &self.head {
                fs::write(
                    format!("{}/.git/HEAD", self.git_dir),
                    format!("ref: {}", ref_name),
                )
                .unwrap();
            }
            let p = format!("{}/.git/{}", self.git_dir, ref_name);
            let p = path::Path::new(&p);
            let parent = p.parent().unwrap();
            if !parent.exists() {
                fs::create_dir_all(p.parent().unwrap()).unwrap();
            }

            let mut path = p.to_str().unwrap().to_string();
            let mut content = ref_hash.clone();
            // println!("{:?}", parent);
            if ref_name.starts_with("refs/remotes") {
                path = format!("{}/HEAD", parent.to_str().unwrap());
                content = format!("ref: {}", ref_name);
            }

            fs::write(path, content).unwrap();
        });
    }
    fn persist_objects(&mut self) {
        let object_dir = format!("{}/.git/objects", self.git_dir);
        if fs::create_dir_all(&object_dir).is_err() {
            panic!("Failed to create .git/objects directory");
        }

        self.objects
            .iter()
            .for_each(|(_, obj)| obj.persist(&object_dir));
    }
}

pub fn get_objects(
    git_url: &str,
    hashes: Vec<String>,
) -> Result<HashMap<String, GitObject>, Box<dyn std::error::Error>> {
    let mut objects = HashMap::new();
    let mut wants = hashes
        .iter()
        .map(|x| {
            let want = format!("want {}", x);
            let length = want.len() + 5;
            format!("{:04x}{}", length, want)
        })
        .collect::<Vec<String>>();
    wants.dedup();
    let wants = wants[0..].join("\n");
    let client = reqwest::Client::new();
    let body = format!("{}{}", wants, "\n00000009done\n");
    let url = format!("{}/git-upload-pack", git_url);
    let res = client
        .post(url)
        .header("Content-Type", "application/x-git-upload-pack-request")
        .body(body)
        .send()?;

    let res_bytes = res.bytes()?;

    let mut reader = BufReader::new(&res_bytes[..]);

    let mut bytes = vec![0; 8];
    reader.read_exact(&mut bytes).unwrap();

    let mut pack = vec![0; 4];
    reader
        .read_exact(&mut pack)
        .expect("invalid packfile signature");
    // println!("{:?}", String::from_utf8_lossy(&pack));
    // ignore version
    reader
        .read_exact(&mut pack)
        .expect("invalid packfile version");
    // println!("{:?}", String::from_utf8_lossy(&pack));

    let mut number_of_objects = [0; 4];
    reader
        .read_exact(&mut number_of_objects)
        .expect("invalid number of objects");
    // number_of_objects
    //     .iter()
    //     .for_each(|b| println!("byte: {:02x}", b));
    let number_of_objects = u32::from_be_bytes(number_of_objects);
    // println!("number_of_objects: {}", number_of_objects);

    for _ in 0..number_of_objects {
        let (object_type, object_size) = parse_object_header(&mut reader);
        let mut base_object_bin_hash = vec![0u8; 20];
        let mut base_object_hash = String::new();

        if object_type == 7 {
            reader
                .read_exact(&mut base_object_bin_hash)
                .expect("invalid base object hash");
            base_object_hash = hex::encode(&base_object_bin_hash);
        }

        let mut object = {
            let object_size = if object_size > 0 { object_size } else { 1 };
            let mut object = vec![0u8; object_size as usize];
            let mut decompressor = flate2::bufread::ZlibDecoder::new(&mut reader);
            if decompressor.read_exact(&mut object).is_err() {}
            object
        };

        if object_type != 7 {
            if object_size == 0 {
                object = vec![];
            }
            let object = GitObject::new(object, object_type.into());
            objects.insert(object.id.clone(), object);
        } else {
            // println!("base_object_hash: {}", base_object_hash);

            if let Some(base_object) = objects.get(&base_object_hash) {
                let object = reconstruct_object(object, base_object);

                objects.insert(object.id.clone(), object);
            } else {
                println!("base object not found");
            }
            // println!();
        }
    }

    Ok(objects)
}

fn reconstruct_object(delta_object: Vec<u8>, base_object: &GitObject) -> GitObject {
    let mut reader = BufReader::new(delta_object.as_slice());
    let _base_object_size = parse_size_encoding(&mut reader, 0);
    let _target_object_size = parse_size_encoding(&mut reader, 0);
    let base_object_content = &base_object.content;

    let mut target_object: Vec<u8> = vec![];
    loop {
        let mut byte = vec![0; 1];
        if reader.read_exact(&mut byte).is_err() {
            break;
        }
        let msb = byte[0] >> 7;
        // println!("msb: {} - instruction byte: {:08b}", msb, byte[0]);
        if msb == 1 {
            let mut size = 0;
            let mut offset: u32 = 0;
            let offset_bitmask = byte[0] & 0b1111;
            let size_bitmask = (byte[0] >> 4) & 0b111;
            // println!("offset_bitmask: {:08b}", offset_bitmask);
            // println!("size_bitmask: {:08b}", size_bitmask);
            let mut offset_bytes = vec![];
            let mut size_bytes = vec![];

            for i in 0..4 {
                // println!("{}: {}", i, offset_bitmask & (1 << i));
                if offset_bitmask & (1 << i) == 0 {
                    offset_bytes.push(0);
                } else {
                    reader.read_exact(&mut byte).expect("invalid offset bytes");
                    // println!("read offset byte: {:02x}", byte[0]);
                    let byte = byte[0] as u32;
                    offset += byte << (i * 8);
                }
            }
            // println!("offset : {} - 0x{:08x}", offset, offset);
            for i in 0..3 {
                // println!("{}: {}", i, size_bitmask & (1 << i));
                if size_bitmask & (1 << i) == 0 {
                    size_bytes.push(0);
                } else {
                    reader.read_exact(&mut byte).expect("invalid size bytes");
                    // println!("read size byte: {:02x}", byte[0]);
                    let byte = byte[0] as u32;
                    size += byte << (i * 8);
                }
            }

            // println!(
            //     "offset bytes: {}",
            //     offset_bytes
            //         .iter()
            //         .map(|x| format!("{:02x}", x))
            //         .collect::<String>()
            // );
            if size == 0 {
                size = 0x10000;
            }

            target_object.extend(&base_object_content[offset as usize..(offset + size) as usize]);
        } else {
            let size = byte[0] & 0x7f;
            let mut add_object = vec![0; size as usize];
            reader
                .read_exact(&mut add_object)
                .expect("invalid data object for delta insert instruction");
            target_object.extend(&add_object);
        }
    }
    let output = GitObject::new(target_object, base_object.object_type.clone());
    // println!("output id = {}", output.id);
    // println!("output type = {}", output.object_type);
    // println!("output content = {}", String::from_utf8_lossy(&output.content));
    output
}

fn parse_tree_object(content: &[u8]) -> Vec<(String, String, String)> {
    let mut reader = BufReader::new(content);

    let mut i = 0;
    let mut result = vec![];

    loop {
        let mut bytes = vec![];
        let read_result = reader.read_until(b'\0', &mut bytes);
        if let Ok(l) = read_result {
            if l > 20 {
                bytes.pop().unwrap();
            } else if l == 0 {
                break;
            }
        } else {
            break;
        }

        if i == 0 {
            let (mode, name) = std::str::from_utf8(&bytes)
                .unwrap()
                .split_once(' ')
                .unwrap();
            result.push((mode.to_string(), name.replace('\0', "").to_string(), "".into()));
        } else {
            while bytes.len() <= 20 {
                let mut next_part = vec![];
                if let Ok(l) = reader.read_until(b'\0', &mut next_part) {
                    if l == 0 {
                        break;
                    }
                } else {
                    break;
                }
                bytes = [bytes, next_part].concat();
            }

            let sha = &bytes[0..20];
            result[i - 1].2 = hex::encode(sha);

            if let Some((mode, name)) = std::str::from_utf8(&bytes[20..]).unwrap().split_once(' ') {
                result.push((mode.to_string(), name.replace('\0', "").to_string(), "".into()))
            };
        }
        i += 1;
    }

    result
}

fn parse_object_header<T: Read>(reader: &mut T) -> (u8, u128) {
    let mut first_byte = [0; 1];
    let _ = reader.read_exact(&mut first_byte);
    let obj_type = (first_byte[0] >> 4) & 0x07;
    let mut object_size = (first_byte[0] & 0xF) as u128;
    let msb = first_byte[0] >> 7;
    if msb == 1 {
        object_size = parse_size_encoding(reader, object_size as u32);
    }

    (obj_type, object_size)
}

fn parse_size_encoding<T: Read>(reader: &mut T, base_size: u32) -> u128 {
    let mut object_size = base_size as u128;
    let mut msb = 1;

    let mut c = 0;
    while msb != 0 {
        let mut first_byte = [0; 1];
        let result = reader.read_exact(&mut first_byte);
        msb = first_byte[0] >> 7;
        let current_byte: u128 = (first_byte[0] & 0b0111_1111) as u128;
        object_size = (current_byte << (4 + 7 * c)) + (object_size);
        c += 1;
    }

    object_size
}

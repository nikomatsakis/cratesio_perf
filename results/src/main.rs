extern crate regex;

fn match_times(text: &str) -> std::io::Result<Vec<String>> {
    use std::io::{Error, ErrorKind};
    use regex::Regex;

    let mut output = vec![];
    for line in text.lines() {
        if line == "OK" {
            return Ok(output);
        }
        let re = Regex::new(r"^time:\s(\d+\.\d+).+$").unwrap();
        for cap in re.captures_iter(line) {
            match cap.at(1) {
                Some(s) => output.push(s.to_string()),
                None => return Err(Error::new(ErrorKind::Other, "bad compile"))
            }
        }
    }
    Err(Error::new(ErrorKind::Other, "bad compile"))
}
fn simple_print(ve: Vec<String>) {
    let mut first = true;
    for v in ve {
        if first {
            first = false;
            print!("{}", v);
        }
        else {
            print!(", {}", v);
        }
    }
    println!("");
}

fn readdir(dir: &str) -> std::io::Result<()> {
    use std::fs::{self, File};
    use std::io::prelude::*;

    for entry in try!(fs::read_dir(dir)) {
        let dir = try!(entry);
        //println!("{:?}", dir.path());

        let mut f = try!(File::open(dir.path().join("stdio")));
        let mut s = String::new();
        try!(f.read_to_string(&mut s));

        match match_times(&s) {
            Ok(mut v) => {
                v.insert(0, "true".to_string());
                v.insert(0, dir.path().to_str().unwrap().to_string());
                simple_print(v);
            }
            Err(_) => {
                let v = vec![dir.path().to_str().unwrap().to_string(), "false".to_string()];
                simple_print(v);
            }
        }
    }
    Ok(())
}

fn main() {
    use std::env;

    for arg in env::args() {
        &readdir(&arg);
    }
}

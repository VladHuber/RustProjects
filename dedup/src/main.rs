use std::fs::{self, File};
use std::io;
use std::io::Read;
use std::{error::Error, path::PathBuf};
use walkdir::WalkDir;

fn main() {
    find_duplicates().unwrap();
}

fn find_duplicates() -> Result<(), Box<dyn Error>> {
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new("foo") {
        let entry = entry?;
        if entry.path().is_file() {
            files.push(entry.path().to_path_buf());
        }
    }
    let mut delete_files: Vec<&str> = Vec::new();
    for file1 in files.iter() {
        let filename1: &str = file1.file_name().unwrap().to_str().unwrap();
        let found_deleted = delete_files.iter().find(|&&x| x == filename1);

        if found_deleted.is_some() {
            continue;
        }
        let shafile1 = shafile(file1.to_path_buf());

        for file2 in files.iter() {
            let filename2 = file2.file_name().unwrap().to_str().unwrap();
            let found_deleted = delete_files.iter().find(|&&x| x == filename2);
            if found_deleted.is_some() {
                continue;
            }
            if filename1 == filename2 {
                continue;
            }
            let shafile2 = shafile(file2.to_path_buf());

            if shafile1 == shafile2 {
                println!("File is duplicate {}, {}", filename1, filename2);
                println!("SHA is duplicate {}, {}", shafile1, shafile2);

                println!("Удалить файл? Y - Да, N - Нет");
                let mut guess = String::new();
                io::stdin()
                    .read_line(&mut guess)
                    .expect("Failed to read line");
                println!("{}", guess == "Y");
                if guess.trim() == "Y" {
                    fs::remove_file(file2).unwrap();
                    println!("Файл удален {}", filename2);
                    delete_files.push(filename2);
                }
            }
        }
    }
    Ok(())
}

fn shafile(path: PathBuf) -> String {
    let mut file = File::open(path).unwrap();
    let mut buffer = [0; 1024];
    let mut hasher = blake3::Hasher::new();
    loop {
        let bytes_read = file.read(&mut buffer).unwrap();
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    hasher.finalize().to_string()
}

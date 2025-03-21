use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Represents a text file with a filename and content
pub struct TextFile {
    pub filename: String,
    pub content: String,
}

/// Creates a directory if it doesn't exist and optionally writes a vector of TextFiles to it
///
/// # Arguments
///
/// * `directory_path` - Path of the directory to create
/// * `files` - Optional vector of TextFile structs to write to the directory
///
/// # Returns
///
/// * `io::Result<()>` - Success or an IO error
///
/// # Examples
///
/// ```
/// let files = vec![
///     TextFile {
///         filename: "example.txt".to_string(),
///         content: "Hello, world!".to_string(),
///     },
///     TextFile {
///         filename: "config.txt".to_string(),
///         content: "debug=true\nverbose=false".to_string(),
///     },
/// ];
///
/// create_directory_with_files("my_directory", Some(files))?;
/// ```
pub fn create_directory_with_files(
    directory_path: &Path,
    files: Option<Vec<TextFile>>,
) -> io::Result<()> {

    if !directory_path.exists() {
        fs::create_dir_all(directory_path)?;
        println!("Created directory: {:?}", directory_path);
    }

    // If files were provided, write them to the directory
    if let Some(text_files) = files {
        for file in text_files {
            let file_path = directory_path.join(&file.filename);
            if(!file_path.exists()) {
                let mut file_handle = fs::File::create(file_path)?;
                file_handle.write_all(file.content.as_bytes())?;
                println!("Created file: {}", file.filename);
            }
        }
    }

    Ok(())
}
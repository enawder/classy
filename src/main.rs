use std::path::PathBuf;
use std::string::String;

extern crate serde;
extern crate yaml_rust;
extern crate pdf;
extern crate preferences;
extern crate directories;

use anyhow::Context;
use clap::Parser;
use directories::ProjectDirs;
// use preferences::{AppInfo, PreferencesMap, Preferences};
// use serde::{Serialize, Deserialize};
use walkdir::WalkDir;
use yaml_rust::YamlLoader;
use yaml_rust::yaml;

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    #[clap(
        short,
        long,
        parse(from_os_str)
    )]
    /// Input directory containing files to be classified.
    input: std::path::PathBuf,

    #[clap(
        short,
        long,
        parse(from_os_str)
    )]
    /// Output directory.
    output: std::path::PathBuf,

    #[clap(
        long,
        parse(from_os_str)
    )]
    config: Option<std::path::PathBuf>,

    #[clap(long)]
    /// Display configuration file.
    print_config: bool
}

#[derive(Default)]
struct ClassifierPath {
    path: std::path::PathBuf,
    keywords: Vec<String>
}
type ClassifierPaths = Vec<ClassifierPath>;

impl ClassifierPath {
    fn matches(&self, text: &str) -> bool {
        let contains = self.keywords.iter().all(|word| {
            regex::Regex::new(&["\\b", word, "\\b"].join(""))
                .unwrap()
                .is_match(text)
        });
        !self.keywords.is_empty() && contains
    }    
}

impl std::fmt::Display for ClassifierPath {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "(path: {:?}, keywords: {:?})", self.path, self.keywords)
    }   
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut config_path = PathBuf::new();
    if let Some(path) = args.config {
        config_path = path.clone();
    } else if let Some(proj_dirs) = ProjectDirs::from("", "", "ddc") {
        config_path = proj_dirs.config_dir().join("config.yml");
    }

    if args.print_config {
        print_config(&config_path)?;
        return Ok(())
    }

    let config = parse_config(&config_path)?;

    let extensions: std::collections::HashSet<&str>
        = vec!["pdf"].into_iter().collect();
    let files = WalkDir::new(args.input)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            let extension = e.path().extension();
            if extension == None { return false; };
            let extension = extension.unwrap().to_str().unwrap();
            e.file_type().is_file() && extensions.contains(&extension)
    });
    for file in files
    {
        classify(&file, &config);
    }
    Ok(())
}

fn is_pdf(file: &walkdir::DirEntry) -> bool {
    file.path().extension().unwrap().to_str().unwrap() == "pdf"
}

fn classify(file: &walkdir::DirEntry, config: &ClassifierPaths) {
    if is_pdf(file) {
        classify_pdf(&file, &config);
    }
}

fn classify_pdf(file: &walkdir::DirEntry, config: &ClassifierPaths) {
    let doc = poppler::PopplerDocument::new_from_file(
        file.path(),
        std::path::Path::new("").to_str().unwrap()).unwrap();
    let page = doc.get_page(0).unwrap();
    let text = page.get_text().unwrap();
    let matches: Vec<&ClassifierPath> =
        config.iter().filter(|path| path.matches(text)).collect();
    if !matches.is_empty() {
        println!(" src: {}", file.path().to_str().unwrap());
        for m in matches.iter() {
            println!("dest: {:?} using keywords: {:?}", m.path, m.keywords);
        }
        println!("");
    }
}

fn config_to_str(path: &std::path::PathBuf) -> anyhow::Result<String> {
    return std::fs::read_to_string(&path).with_context(|| {
        format!("Failed to read configuration file '{}'",
            path.to_str().unwrap())
    });
}

fn print_config(path: &std::path::PathBuf) -> anyhow::Result<()> {
    println!("{}", config_to_str(&path)?);
    Ok(())
}

fn parse_config(path: &std::path::PathBuf) -> anyhow::Result<ClassifierPaths> {
    let config = config_to_str(&path)?;
    let config = YamlLoader::load_from_str(&config)
        .with_context(|| {
            format!("Failed to parse configuration file '{}'",
                path.to_str().unwrap())
        })?;
    let root = config.first().with_context(|| {
        "No root element found"
    })?;
    let config = root.as_vec().context(
        "Unexpected configuration file format"
    )?;

    let config = parse_layout(config)?;
    for i in config.iter() {
        println!("{}", i);
    }
    Ok(config)
}

fn parse_layout(layout: &yaml::Array) -> anyhow::Result<ClassifierPaths> {
    let dir_key = yaml::Yaml::from_str("dir");
    let sub_key = yaml::Yaml::from_str("sub");
    let keywords_key = yaml::Yaml::from_str("keywords");
    let mut paths: ClassifierPaths = Default::default();

    for dir in layout.iter() {
        let dir_params = dir.as_hash().context(
            "Unexpected configuration file format. Expected a hash map."
        )?;
        let dir_name = dir_params.get(&dir_key).context(
            format!("No '{}' key found !", dir_key.as_str().unwrap())
        )?;
        let mut path: ClassifierPath = Default::default();
        path.path = std::path::PathBuf::from(dir_name.as_str().unwrap());
        if let Some(keywords) = dir_params.get(&keywords_key) {
            let keywords = keywords.as_vec().context(
                format!("Unexpected keywords format for directory {:?}", path.path)
            )?;
            path.keywords = keywords.into_iter().map(|yaml| {
                yaml.as_str().unwrap().to_string()
            }) .collect();
        }
        let new_path = ClassifierPath{
            path: path.path.clone(),
            keywords: path.keywords.clone()
        };
        paths.push(new_path);
        if !dir_params.contains_key(&sub_key) {
            continue;
        }
        let sub_dirs = dir_params[&sub_key].as_vec().with_context(||
            format!("'{}' element should be a list of directories",
                sub_key.as_str().unwrap())
        )?;
        let mut sub = parse_layout(sub_dirs)?;
        for it in sub.iter_mut() {
            let mut clone = path.path.clone();
            clone.push(it.path.clone());
            it.path = clone.clone();
            it.keywords.extend(path.keywords.clone());
        }
        paths.extend(sub);
    }
    Ok(paths)
}

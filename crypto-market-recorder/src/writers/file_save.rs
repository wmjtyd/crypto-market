use chrono::prelude::Local;
use chrono::Duration;
use std::collections::HashMap;
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

pub struct WriteData {
    is_runing: Arc<Mutex<bool>>,
    channel_sender: mpsc::Sender<(String, Vec<i8>)>,
    channel_receiver: Arc<Mutex<mpsc::Receiver<(String, Vec<i8>)>>>,
    files: Arc<Mutex<HashMap<String, File>>>,
}

impl WriteData {
    pub fn new() -> WriteData {
        let (channel_sender, channel_receiver) = mpsc::channel::<(String, Vec<i8>)>();
        WriteData {
            is_runing: Arc::new(Mutex::new(true)),
            channel_sender,
            channel_receiver: Arc::new(Mutex::new(channel_receiver)),
            files: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn add_order_book(&mut self, filename: String, array: Vec<i8>) {
        self.channel_sender
            .send((filename, array))
            .expect("文件数据信道出现问题")
    }

    pub fn start(&mut self) -> JoinHandle<()> {
        let is_runing = self.is_runing.clone();
        let channel_receiver = self.channel_receiver.clone();
        let files = self.files.clone();
        thread::spawn(move || loop {
            println!("hello");
            {
                let is_runing = is_runing.lock();
                match is_runing {
                    Ok(_is_runing) => {
                        if !*_is_runing {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let local_time = Local::now();
            let today = local_time.format("%Y%m%d").to_string();

            let mut filename: String = "".to_string();
            let mut data: Vec<i8> = vec![];

            {
                let channel_receiver_lock = channel_receiver.lock();
                let mut flag = false;
                if let Ok(channel_receiver_look) = channel_receiver_lock {
                    if let Ok(receiver) = channel_receiver_look.recv() {
                        filename = receiver.0;
                        data = receiver.1;
                        flag = true;
                    }
                }
                if !flag {
                    break;
                }
            }

            let mut files_lock = if let Ok(files_lock) = files.lock() {
                files_lock
            } else {
                continue;
            };

            match files_lock.get(&format!("{}_{}", filename, local_time)) {
                None => {

                    let filename_orderbook = check_path(filename.to_string());
                    let data_file = OpenOptions::new()
                        .append(true)
                        .open(filename_orderbook)
                        .expect("文件无法打开");

                    write_file(&data_file, data);
                    files_lock.insert(today, data_file);

                    let yesterday = (local_time - Duration::seconds(86400)).format("%Y%m%d");
                    let key = &format!("{}_{}", filename, yesterday);
                    if files_lock.contains_key(key) {
                        files_lock.remove(key);
                    }
                }
                Some(file) => {
                    write_file(file, data);
                }
            };
        })
    }
}

pub struct ReadData {
    file: File,
}

impl ReadData {
    pub fn new(filename: String, day: i64) -> Option<ReadData> {
        let local_time = Local::now();
        let day_time = local_time - Duration::seconds(86400 * day);
        let day_format = day_time.format("%Y%m%d");

        let path = &format!("./record/{}/", day_format);

        let path_filename = format!("{}{}.csv", path, filename);

        println!("{}", path_filename);
        if !Path::new(&path_filename).is_file() {
            // 文件不存在
            return None;
        }

        let file = File::open(path_filename).unwrap();

        Some(ReadData { file })
    }
}

// http utp utp:quic
impl Iterator for ReadData {
    type Item = Vec<i8>;

    // 可优化
    fn next(&mut self) -> Option<Self::Item> {
        let mut data_len = [0u8; 2];
        if let Ok(_data_size) = self.file.read(&mut data_len) {
            if 2 != _data_size {
                return None;
            }
            let mut data: Vec<i8> = Vec::new();
            let data_len = u16::from_be_bytes(data_len) as usize;
            let mut byte = [0u8; 1];
            for _i in 0..data_len {
                if let Result::Err(_error_msg) = self.file.read(&mut byte) {
                    return None;
                }
                data.push(byte[0] as i8);
            }
            return Some(data);
        }
        None
    }
}

// 可优化
pub fn write_file(mut file: &File, data: Vec<i8>) {
    let data_len = &(data.len() as i16).to_be_bytes();
    file.write_all(data_len).expect("长度计算失败");
    for i in data {
        file.write_all(&[i as u8]).expect("");
    }
}

pub fn check_path(filename: String) -> String {
    let local_time = Local::now().format("%Y%m%d").to_string();
    let path = &format!("./record/{}/", local_time);
    create_dir_all(path).expect("目录创建失败");

    let path_filename = format!("{}{}.csv", path, filename);
    println!("{}", path_filename);
    if !Path::new(&path_filename).is_file() {
        File::create(&path_filename).expect("创建文件失败");
    }
    path_filename
}

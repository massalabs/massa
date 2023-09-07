use std::{path::PathBuf, io::{BufReader, Lines}, fs::File, unreachable, unimplemented};

use clap::Parser;
use std::io::BufRead;

#[derive(Parser)]
struct Args {
    #[arg(short='0', long)]
    labnet0: PathBuf,

    #[arg(short='1', long)]
    labnet1: PathBuf,
}

pub struct LogsInspector {
    lines_0: Lines<BufReader<File>>,
    lines_1: Lines<BufReader<File>>,
    filter_tags: Vec<String>,
}

impl LogsInspector {
    pub fn new(l0: BufReader<File>, l1: BufReader<File>) -> LogsInspector {
        // get_next_slot(lines_iter_0);
        LogsInspector {
            lines_0: l0.lines(),
            lines_1: l1.lines(),
            filter_tags: vec![
                "TESTNETDBG".to_string(),
                "final_state hash".to_string(),
            ],
        }
    }

    fn catch_number(&self, line: &str) -> String {
        let mut res = String::new();
        let mut reached_digit = false;
        for c in line.chars() {
            if c.is_digit(10) {
                reached_digit = true;
                res.push(c);
            } else if reached_digit {
                break;
            }
        }
        assert!(!res.is_empty());
        res
    }

    fn try_get_slot(&self, line: &String) -> Option<(u64, u64)> {
        let contains_filter_tags = self.filter_tags.iter().any(|tag| line.contains(tag));
        if !line.contains("period:") || !line.contains("thread:") || !contains_filter_tags {
            None           
        } else {
            let mut workline = line.split("period:").nth(1).unwrap().split(',');
            let period : u64 = self.catch_number(workline.nth(0).unwrap()).parse().unwrap();
            let thread : u64 = self.catch_number(workline.nth(0).unwrap()).parse().unwrap();
            Some((period, thread))
        }
    }

    fn find_next_slot(&mut self, idx: usize) -> Option<(String, (u64, u64))> {
        loop {
            let reader = match idx {
                0 => &mut self.lines_0,            
                1 => &mut self.lines_1,
                _ => unreachable!(),
            };
            let l0 = match reader.next() {
                None => return None,
                Some(line) => line.expect("Error while getting line of labnet0"),
            };

            if let Some(slot) = self.try_get_slot(&l0) {
                return Some((l0, slot));
            }
        }
    }

    fn compare_slots(&self, slot0: (u64, u64), slot1: (u64, u64)) -> Option<usize> {
        match slot0.0.cmp(&slot1.0) {
            std::cmp::Ordering::Less => Some(0),
            std::cmp::Ordering::Greater => Some(1),
            std::cmp::Ordering::Equal => match slot0.1.cmp(&slot1.1) {
                std::cmp::Ordering::Less => Some(0),
                std::cmp::Ordering::Greater => Some(1),
                std::cmp::Ordering::Equal => None,
            },
        }
    }

    fn get_next(&mut self) -> Option<(String, String, (u64, u64))> {
        let Some((mut l0, mut slot0)) = self.find_next_slot(0) else {
            return None;
        };
        let Some((mut l1, mut slot1)) = self.find_next_slot(1) else {
            return None;
        };

        let mut slot_cmp = self.compare_slots(slot0, slot1);

        while let Some(fwd_idx) = slot_cmp {
            let Some((next_line, next_slot)) = self.find_next_slot(fwd_idx) else {
                return None;
            };

            if fwd_idx == 0 {
                slot0 = next_slot;
                l0 = next_line;
            } else {
                slot1 = next_slot;
                l1 = next_line;
            }

            slot_cmp = self.compare_slots(slot0, slot1);
        }

        Some((l0, l1, slot0))
    }

    pub fn inspect(mut self) {
        loop {
            let Some((l0, l1, slot)) = self.get_next() else {
                println!("Reached end of one of the files");
                break;
            };

            self.inspect_lines(slot, l0, l1);
            // let slot = self.try_get_slot(l0)
        }
    }

    fn inspect_lines(&self, slot: (u64, u64), line0: String, line1: String) {
        let mut case = None;
        for (n, tag) in self.filter_tags.iter().enumerate() {
            if line0.contains(tag) {
                if !line0.contains(tag) {
                    panic!("Desynchro lines");
                }
                case = Some(n);
            }
        }
        let Some(case) = case else {
            return;
        };

        match case {
            0 => self.db_dump_debug(slot, line0, line1),
            1 => self.final_state_hash_debug(slot, line0, line1),
            _ => unimplemented!(),
        }
    }

    fn db_dump_debug(&self, slot: (u64, u64), l0: String, l1: String) {
        if l0 != l1 {
            println!("Different line");
        }
    }

    fn final_state_hash_debug(&self, slot: (u64, u64), l0: String, l1: String) {
        let hash0 = l0.split(" ").last().unwrap();
        let hash1 = l1.split(" ").last().unwrap();
        if hash0 != hash1 {
            println!("{slot:?} - hash {hash0} {hash1} - different");
        }
    }
}

fn main() {
    let args = Args::parse();

    let f0 = BufReader::new(File::open(args.labnet0).expect("Unable to open labnet0 file"));
    let f1 = BufReader::new(File::open(args.labnet1).expect("Unable to open labnet1 file"));

    let inspector = LogsInspector::new(f0, f1);
    inspector.inspect();
}

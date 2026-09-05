// Standalone subprocess fixture compiled by solver_process_tests. Deliberately
// uses no shell so paths with spaces and argument boundaries are exercised.
use std::{env, fs, io::Write, path::Path, process::Command, time::Duration};

fn main() {
    let exe = env::current_exe().unwrap();
    let role = exe.file_stem().unwrap().to_str().unwrap();
    let mut order = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("order.log")
        .unwrap();
    writeln!(order, "{role}").unwrap();
    match role {
        "hecmw_part1" => {
            fs::write("partition-started", b"started").unwrap();
            if Path::new("sleep-partition").exists() {
                std::thread::sleep(Duration::from_secs(30));
            }
            if Path::new("fail-partition").exists() {
                eprintln!("partition-error");
                std::process::exit(7);
            }
            let ctrl = fs::read_to_string("hecmw_ctrl.dat").unwrap();
            let part = fs::read_to_string("hecmw_part_ctrl.dat").unwrap();
            let ranks: usize = part
                .split("DOMAIN=")
                .nth(1)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            let mut lines = ctrl.lines();
            let prefix = loop {
                let line = lines.next().unwrap();
                if line.contains("NAME=part_out") {
                    break lines.next().unwrap().trim();
                }
            };
            assert!(ctrl.contains("NAME=fstrMSH, TYPE=HECMW-DIST"));
            for rank in 0..ranks {
                if rank == ranks - 1 && Path::new("missing-partition").exists() {
                    continue;
                }
                fs::write(format!("{prefix}.{rank}"), b"fresh partition").unwrap();
            }
            println!("partition-ok");
        }
        "mpiexec" => {
            let args: Vec<_> = env::args_os().skip(1).collect();
            assert_eq!(args.len(), 3);
            assert_eq!(args[0], "-n");
            let part = fs::read_to_string("hecmw_part_ctrl.dat").unwrap();
            assert_eq!(part.split("DOMAIN=").nth(1).unwrap().trim(), args[1]);
            let status = Command::new(&args[2]).status().unwrap();
            std::process::exit(status.code().unwrap_or(1));
        }
        "fistr1" => {
            fs::write("solver-called", b"yes").unwrap();
            println!("solver-ok");
        }
        "tree-parent" => {
            let child = Command::new(exe.with_file_name(if cfg!(windows) {
                "tree-child.exe"
            } else {
                "tree-child"
            }))
            .spawn()
            .unwrap();
            fs::write("child-pid", child.id().to_string()).unwrap();
            std::thread::sleep(Duration::from_secs(30));
        }
        "tree-child" => {
            std::thread::sleep(Duration::from_secs(30));
        }
        _ => panic!("unknown fixture role: {role}"),
    }
}

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
            assert_eq!(args.len(), 5);
            assert_eq!(args[0], "-n");
            let part = fs::read_to_string("hecmw_part_ctrl.dat").unwrap();
            assert_eq!(part.split("DOMAIN=").nth(1).unwrap().trim(), args[1]);
            assert_eq!(args[3], "-t");
            assert_eq!(args[4], "1");
            let status = Command::new(&args[2]).args(&args[3..]).status().unwrap();
            std::process::exit(status.code().unwrap_or(1));
        }
        "fistr1" | "bar-solver" => {
            fs::write("solver-called", b"yes").unwrap();
            if Path::new("live-output").exists() {
                println!("running-before-completion");
                std::io::stdout().flush().unwrap();
                while !Path::new("allow-finish").exists() {
                    std::thread::sleep(Duration::from_millis(20));
                }
                eprintln!("diagnostic-at-completion");
            }
            if role == "bar-solver" {
                write_bar_results();
            }
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

fn write_bar_results() {
    let control = fs::read_to_string("hecmw_ctrl.dat").unwrap();
    let mut lines = control.lines();
    let result_header = loop {
        if lines.next().unwrap().starts_with("!RESULT, NAME=fstrRES,") {
            break lines.next().unwrap().trim();
        }
    };
    let cnt = fs::read_to_string("bar.cnt").unwrap();
    let mut loads = Vec::new();
    let mut in_load = false;
    for line in cnt.lines().map(str::trim) {
        if line.starts_with('!') {
            in_load = line.starts_with("!CLOAD");
        } else if in_load && !line.is_empty() {
            let force: f32 = line.rsplit(',').next().unwrap().trim().parse().unwrap();
            loads.push(force);
        }
    }
    assert_eq!(loads.len(), 4);
    for step in 0..=1 {
        let mut result = fs::File::create(format!("{result_header}.0.{step}")).unwrap();
        writeln!(result, "*fstrresult\n20 0\n1 0\n3\nDISPLACEMENT").unwrap();
        // Reverse file order to exercise the actual node-ID remapping.
        for id in (1..=20).rev() {
            let x = (id - 1) / 4;
            let ux = loads.iter().sum::<f32>() * x as f32 * step as f32 / 1000.0;
            writeln!(result, "{id}\n{ux:.8e} 0 0").unwrap();
        }
    }
}

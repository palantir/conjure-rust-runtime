// Copyright 2020 Palantir Technologies, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
use crate::simulation::{Simulation, SimulationBuilder1, SimulationReport, Strategy};
use std::any;
use std::env;
use std::fs;
use std::io::BufWriter;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Harness {
    results: Vec<SimulationResult>,
}

impl Harness {
    pub fn new() -> Self {
        Harness { results: vec![] }
    }

    pub fn simulation<F>(mut self, f: F) -> Self
    where
        F: Fn(SimulationBuilder1) -> Simulation,
    {
        // a bit hacky, but avoids some duplication and this is just test code
        let name = any::type_name::<F>().split(':').last().unwrap();

        for &strategy in &[
            Strategy::ConcurrencyLimiterRoundRobin,
            Strategy::ConcurrencyLimiterPinUntilError,
            Strategy::UnlimitedRoundRobin,
        ] {
            let report = f(Simulation::builder().strategy(strategy)).run();
            let result = SimulationResult {
                name,
                strategy,
                report,
            };
            self.results.push(result);
            let result = &self.results[self.results.len() - 1];

            let txt_file = self.txt_file(result);

            let old_summary = if txt_file.exists() {
                fs::read_to_string(&txt_file).unwrap()
            } else {
                String::new()
            };

            let new_summary = result.summary();

            if new_summary == old_summary {
                continue;
            }

            if env::var_os("CI").is_some() {
                panic!(
                    "simulation {} results have changed - rerun simulations locally and check in the new results",
                    result.basename(),
                );
            }

            fs::write(txt_file, new_summary).unwrap();

            let image_path = self.png_file(result);
            if image_path.exists() {
                let prev_path = image_path.with_extension("prev.png");
                fs::rename(&image_path, prev_path).unwrap();
            }
            result
                .chart(&image_path)
                .expect("error rendering charts with gnuplot");
        }

        self
    }

    pub fn finish(mut self) {
        if env::var_os("CI").is_some() {
            return;
        }

        self.results.sort_by_key(|r| r.basename());

        let report = format!(
            "
# Report
<!-- Run `cargo test -p simulation --release` to regenerate this report. -->

{}
{}
            ",
            self.report_txt_section(),
            self.report_images_section(),
        );

        fs::write(self.results_dir().join("report.md"), report).unwrap();
    }

    fn report_txt_section(&self) -> String {
        let width = self
            .results
            .iter()
            .map(|r| r.basename().len())
            .max()
            .unwrap();

        let lines = self
            .results
            .iter()
            .map(|r| format!("{:>width$}:\t{}", r.basename(), r.summary(), width = width))
            .collect::<String>();

        format!("```\n{}```\n", lines)
    }

    fn report_images_section(&self) -> String {
        self.results.iter().map(|r| {
            format!(
                "
## `{basename}`
<table>
    <tr>
        <th>master</th>
        <th>current</th>
    </tr>
    <tr>
        <td><image width=400 src=\"https://media.githubusercontent.com/media/palantir/conjure-rust-runtime/develop/simulation/results/{basename}.png\" /></td>
        <td><image width=400 src=\"{basename}.png\" /></td>
    </tr>
</table>
",
                basename = r.basename(),
            )
        }).collect()
    }

    fn results_dir(&self) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("results")
    }

    fn txt_file(&self, result: &SimulationResult) -> PathBuf {
        self.results_dir()
            .join("txt")
            .join(format!("{}.txt", result.basename()))
    }

    fn png_file(&self, result: &SimulationResult) -> PathBuf {
        self.results_dir()
            .join(format!("{}.png", result.basename()))
    }
}

struct SimulationResult {
    name: &'static str,
    strategy: Strategy,
    report: SimulationReport,
}

impl SimulationResult {
    fn basename(&self) -> String {
        format!("{}[{}]", self.name, self.strategy)
    }

    fn summary(&self) -> String {
        let status_codes = self
            .report
            .status_codes
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "success={}%\tclient_mean={:?}\tserver_cpu={:?}\tclient_received={}/{}\tserver_resps={}\tcodes={{{}}}\n",
            self.report.success_percentage,
            self.report.client_mean,
            self.report.server_cpu,
            self.report.num_received,
            self.report.num_sent,
            self.report.num_global_responses,
            status_codes,
        )
    }

    fn chart(&self, path: &Path) -> io::Result<()> {
        let mut gnuplot = Command::new("gnuplot")
            .arg("-")
            .stdin(Stdio::piped())
            .spawn()?;

        let mut w = BufWriter::new(gnuplot.stdin.take().unwrap());

        let title = format!(
            "{} success={}% client_mean={:?} server_cpu={:?}",
            self.strategy,
            self.report.success_percentage,
            self.report.client_mean,
            self.report.server_cpu
        );

        write!(
            w,
            r#"
set terminal pngcairo font "sans-serif,8" size 800, 1200
set output "{}"
set multiplot title "{}" font "sans-serif,10" noenhanced layout 2,1
"#,
            path.display(),
            title,
        )?;

        self.report
            .record
            .chart(|id| id.name().ends_with("activeRequests"), &mut w)?;

        self.report
            .record
            .chart(|id| id.name().ends_with("request"), &mut w)?;

        w.flush()?;
        drop(w);

        let status = gnuplot.wait().unwrap();
        if !status.success() {
            panic!("gnuplot returned status {}", status);
        }

        Ok(())
    }
}

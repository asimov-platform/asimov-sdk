// This is free and unencumbered software released into the public domain.

//! Unified minimal progress bar for all downloads.

use hf_hub::api::Progress as HfProgress;
use indicatif::{ProgressBar as IBar, ProgressStyle};

pub struct Progress {
    bar: IBar,
    started: bool,
}

impl Progress {
    pub fn new() -> Self {
        let bar = IBar::new(0);
        bar.set_style(
            ProgressStyle::default_bar()
                .template("{wide_bar} {percent:>3}%")
                .unwrap()
                .progress_chars("█░"),
        );
        Self {
            bar,
            started: false,
        }
    }

    pub(crate) fn init_bytes(&mut self, size: u64, position: u64) {
        if size == 0 {
            return;
        }
        self.bar.set_length(size);
        self.bar.set_position(position);
        self.started = true;
    }
}

impl HfProgress for Progress {
    fn init(&mut self, size: usize, _filename: &str) {
        self.init_bytes(size as u64, 0);
    }

    fn update(&mut self, n: usize) {
        if !self.started {
            return;
        }
        let pos = self.bar.position().saturating_add(n as u64);
        self.bar.set_position(pos);
    }

    fn finish(&mut self) {
        if !self.started {
            return;
        }
        self.bar.finish_and_clear();
        self.started = false;
    }
}

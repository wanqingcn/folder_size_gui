use eframe::egui::{self, ScrollArea, ProgressBar};
use rfd::FileDialog;
use clipboard_win::{set_clipboard, formats};
use trash::delete;
use walkdir::WalkDir;
use rayon::prelude::*;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::sync::Arc;
use std::fs;
use std::thread;
use std::time::Duration;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "Directory Size Analyzer",
        options,
        Box::new(|cc| {
            // Configure fonts
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "NotoSansCJK".to_owned(),
                egui::FontData::from_static(include_bytes!("../fonts/NotoSansCJKsc-Regular.otf")).into(),
            );
            fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap()
                .insert(0, "NotoSansCJK".to_owned());
            cc.egui_ctx.set_fonts(fonts);
            
            Ok(Box::new(MyApp::default()))
        }),
    )
}

#[derive(Default)]
struct MyApp {
    dir_path: String,
    entries: Arc<Mutex<Vec<(String, u64)>>>,
    progress: Arc<AtomicU64>,
    scanning: Arc<AtomicBool>,
    pending_delete: Option<usize>,
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Directory Size Analyzer");
            
            ui.horizontal(|ui| {
                ui.label("Directory:");
                ui.text_edit_singleline(&mut self.dir_path);
                
                if ui.button("📁").on_hover_text("Select folder").clicked() {
                    if let Some(path) = FileDialog::new().pick_folder() {
                        self.dir_path = path.to_string_lossy().replace("\\", "/");
                    }
                }
                
                let scanning = self.scanning.load(Ordering::Relaxed);
                let btn = ui.add_enabled(!scanning, egui::Button::new("Scan"));
                if btn.on_hover_text("Scan directory").clicked() {
                    // Reset state for new scan
                    self.entries.lock().clear();
                    self.progress.store(0, Ordering::Relaxed);
                    self.scanning.store(true, Ordering::Relaxed);
                    
                    let path = self.dir_path.clone();
                    let entries = self.entries.clone();
                    let progress = self.progress.clone();
                    let scanning = self.scanning.clone();
                    
                    thread::spawn(move || {
                        calculate_directory_sizes(&path, entries, progress);
                        scanning.store(false, Ordering::Relaxed);
                    });
                }
            });
            
            let progress_value = self.progress.load(Ordering::Relaxed) as f32 / 100.0;
            if progress_value > 0.0 {
                ui.add(
                    ProgressBar::new(progress_value)
                        .text(format!("Scanning... {:.1}%", progress_value * 100.0))
                );
            }
            
            ScrollArea::vertical().show(ui, |ui| {
                let mut entries = self.entries.lock();
                let _to_remove: Option<usize> = None; // Prefix with underscore since unused
                for (idx, (path, size)) in entries.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(path);
                        ui.separator();
                        ui.label(human_readable_size(*size));
                        
                        // Copy button
                        if ui.button("📋").on_hover_text("Copy path").clicked() {
                            let _ = set_clipboard(formats::Unicode, path);
                        }
                        
                        // Delete button
                        if ui.button("❌").on_hover_text("Delete folder").clicked() {
                            self.pending_delete = Some(idx);
                        }
                    });
                }
                
                // Show confirmation dialog if pending delete
                if let Some(idx) = self.pending_delete {
                    let path = entries.get(idx).map(|(p,_)| p.clone());
                    if let Some(path) = path {
                        egui::Window::new("Confirm Delete").show(ctx, |ui| {
                            ui.label(format!("Are you sure to delete {}?", path));
                            ui.horizontal(|ui| {
                                if ui.button("Cancel").clicked() {
                                    self.pending_delete = None;
                                }
                                if ui.button("Confirm").clicked() {
                                    let path = path.clone();
                                    thread::spawn(move || {
                                        let _ = delete(&path);
                                    });
                                    entries.remove(idx);
                                    self.pending_delete = None;
                                }
                            });
                        });
                    }
                }
            });
        });
    }
}

fn calculate_directory_sizes(path: &str, entries: Arc<Mutex<Vec<(String, u64)>>>, progress: Arc<AtomicU64>) {
    let dir_entries: Vec<_> = WalkDir::new(path)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .collect();

    let total = dir_entries.len() as u64;
    let processed = AtomicU64::new(0);
    
    dir_entries.par_iter().for_each(|entry| {
        let path = entry.path();
        let size = if path.is_dir() {
            WalkDir::new(path).into_iter()
                .filter_map(|e| e.ok())
                .map(|e| fs::metadata(e.path()).map(|m| m.len()).unwrap_or(0))
                .sum()
        } else {
            fs::metadata(path).map(|m| m.len()).unwrap_or(0)
        };
        
        let mut entries = entries.lock();
        entries.push((
            path.as_os_str()
                .to_string_lossy()
                .replace("\\", "/"), 
            size
        ));
        
        let processed = processed.fetch_add(1, Ordering::Relaxed) + 1;
        progress.store(processed * 100 / total, Ordering::Relaxed);
    });

    entries.lock().sort_by(|a, b| b.1.cmp(&a.1));
    progress.store(100, Ordering::Relaxed);
    thread::sleep(Duration::from_millis(500));
}

fn human_readable_size(size: u64) -> String {
    let units = ["B", "KB", "MB", "GB"];
    let mut size = size as f64;
    let mut unit_index = 0;
    
    while size >= 1024.0 && unit_index < 3 {
        size /= 1024.0;
        unit_index += 1;
    }
    
    format!("{:.2} {}", size, units[unit_index])
}

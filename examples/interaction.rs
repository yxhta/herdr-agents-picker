#![allow(dead_code)]

#[path = "../src/app.rs"]
mod app;
#[path = "../src/fuzzy.rs"]
mod fuzzy;
#[path = "../src/herdr.rs"]
mod herdr;
#[path = "../src/ui.rs"]
mod ui;

use std::hint::black_box;
use std::time::Instant;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iterations: u32 = std::env::var("PICKER_BENCH_ITERATIONS")
        .unwrap_or_else(|_| "100".into())
        .parse()?;
    assert!(iterations > 0);
    let counts = std::env::var("PICKER_BENCH_AGENTS").map_or_else(
        |_| Ok(vec![20, 200, 2_000]),
        |count| count.parse().map(|count| vec![count]),
    )?;
    for count in counts {
        assert!(count > 0);
        let agents = (0..count)
            .map(|i| herdr::Agent {
                agent: Some("codex".into()),
                agent_status: Some("idle".into()),
                cwd: Some(format!("/home/test/project-{i}")),
                name: Some(format!("agent-{i}")),
                pane_id: Some(format!("w1:p{i}")),
                terminal_id: None,
                terminal_title_stripped: Some("日本語 Review İ workspace".into()),
                workspace_id: None,
                tab_id: None,
                focused: false,
            })
            .collect();
        let mut app = app::App::new(Some("/home/test".into()), None);
        app.set_agents(agents);
        app.enter_search();
        app.filter = "rev".into();
        app.apply_filter();
        assert_eq!(app.filtered.len(), count);
        let mut terminal = Terminal::new(TestBackend::new(160, 32))?;
        if std::env::args().any(|arg| arg == "--snapshot") {
            for (selected, width, height) in [
                (0, 160, 12),
                (count - 1, 160, 12),
                (0, 160, 12),
                (count / 2, 160, 8),
                (count / 2, 160, 32),
                (count - 1, 160, 32),
                (0, 1, 1),
                (count - 1, 8, 3),
                (count - 1, 160, 12),
            ] {
                app.table_state.select(Some(selected));
                terminal.backend_mut().resize(width, height);
                let frame = terminal.draw(|frame| ui::draw(frame, &mut app))?;
                println!(
                    "{count} {selected} {width} {height} {:?} {:?}",
                    app.table_state, frame.buffer
                );
            }
            app.filter = "agent-1".into();
            app.apply_filter();
            let frame = terminal.draw(|frame| ui::draw(frame, &mut app))?;
            println!("filtered {:?} {:?}", app.table_state, frame.buffer);
            continue;
        }
        for _ in 0..10 {
            terminal.draw(|frame| ui::draw(frame, &mut app))?;
        }
        for mode in ["draw", "search_draw"] {
            let started = Instant::now();
            for _ in 0..iterations {
                if mode == "search_draw" {
                    app.apply_filter();
                }
                let frame = terminal.draw(|frame| ui::draw(frame, &mut app))?;
                black_box(&frame.buffer);
                assert_eq!(app.filtered.len(), count);
            }
            let micros = started.elapsed().as_secs_f64() * 1e6 / f64::from(iterations);
            let screen: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect();
            assert!(screen.contains("Review"));
            assert!(screen.contains(&format!("{count}/{count}")));
            println!(
                "{{\"agents\":{count},\"mode\":\"{mode}\",\"iterations\":{iterations},\"us\":{micros:.3},\"errors\":0}}"
            );
        }
    }
    Ok(())
}

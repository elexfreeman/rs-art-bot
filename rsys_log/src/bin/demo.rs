use rsys_log::{Level, LogBuilder};

fn main() {
    // Демонстрация цветовой схемы gruvbox в консоли: 100 логов подряд.
    rsys_log::set_global_level(Level::Trace);

    println!("--- rsys_log Gruvbox demo (100 логов) ---");
    print_stream(100);
}

fn print_stream(total: usize) {
    for idx in 0..total {
        let level = match idx % 5 {
            0 => Level::Trace,
            1 => Level::Debug,
            2 => Level::Info,
            3 => Level::Warn,
            _ => Level::Error,
        };

        let mut builder = LogBuilder::new(
            "demo",
            format!("mod{}", idx % 4),
            level,
            message_for(level, idx),
        )
        .cid(format!("demo-{idx:03}"))
        .data("iteration", idx.to_string())
        .data("dur_ms", (5 + idx % 25).to_string())
        .data("topic", format!("topic-{}", idx % 7));

        if level == Level::Warn {
            builder = builder.data("retry", format!("{}/5", 1 + idx % 5));
            if idx % 3 == 0 {
                builder = builder.detail("hint: проверить очередь задач");
            }
        }

        if level == Level::Error {
            builder = builder
                .data("code", "E500")
                .detail("stack: fn demo_worker -> process_task")
                .detail("cause: simulated error for демонстрации");
        }

        builder.print();
    }
}

fn message_for(level: Level, idx: usize) -> String {
    match level {
        Level::Trace => format!("Trace событие {}", idx),
        Level::Debug => format!("Диагностика шага {}", idx),
        Level::Info => format!("Операция завершена {}", idx),
        Level::Warn => format!("Предупреждение {}", idx),
        Level::Error => format!("Ошибка обработки {}", idx),
    }
}

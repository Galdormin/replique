use std::{
    fs,
    io::{self, BufRead, Write},
    path::Path,
};

use replique::{
    RepliqueFile,
    dialogue::Value,
    parser::diagnostic::Color,
    vm::{DialogueEvent, DialogueVm, ResumeEvent},
};

/// Read a line from stdin. Returns `None` on EOF (Ctrl-D).
fn read_line() -> Option<String> {
    let mut buffer = String::new();
    match io::stdin().lock().read_line(&mut buffer) {
        Ok(0) => None,
        Ok(_) => Some(buffer),
        Err(e) => panic!("failed to read from stdin: {}", e),
    }
}

fn wait_for_enter() {
    io::stdout().flush().unwrap();

    if read_line().is_none() {
        std::process::exit(0);
    }
}

fn read_choice(count: usize) -> usize {
    loop {
        print!("> ");
        io::stdout().flush().unwrap();

        let Some(input) = read_line() else {
            std::process::exit(0);
        };

        match input.trim().parse::<usize>() {
            Ok(i) if i < count => return i,
            _ => println!("  Invalid, enter a number between 0 et {}.", count - 1),
        }
    }
}

fn main() {
    let path = Path::new("assets/dialogue/custom_command.rep");
    let src = fs::read_to_string(path).unwrap();
    let replique = RepliqueFile::from_source(src).with_path(path);

    if replique.has_errors() {
        eprint!("{}", replique.render_diagnostics(Color::Auto));
        return;
    }

    let Some(dialogue) = replique.dialogue else {
        return;
    };

    let mut vm = DialogueVm::default();

    let mut event = vm.start(dialogue, "start").unwrap();
    while !matches!(event, DialogueEvent::Finished) {
        match event {
            DialogueEvent::Say { speaker, text } => {
                match speaker {
                    Some(s) => println!("{}: {}", s, text),
                    None => println!("{}", text),
                }
                wait_for_enter();
                event = vm.resume(ResumeEvent::Advance).unwrap();
            }
            DialogueEvent::Choices { choices } => {
                for (i, choice) in choices.iter().enumerate() {
                    println!("  {}. {}", i, choice);
                }
                let choice = read_choice(choices.len());
                event = vm.resume(ResumeEvent::Select(choice)).unwrap();
            }
            DialogueEvent::Command { name, args } => {
                let action = match name.as_str() {
                    "add_scene" => "added",
                    "remove_scene" => "remove",
                    _ => {
                        println!("!!!! Uknown command {}", name);
                        break;
                    }
                };
                let chars = args
                    .into_iter()
                    .filter_map(|v| match v {
                        Value::String(c) => Some(c),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                match chars.as_slice() {
                    [] => println!(">> No one was {action} to the scene."),
                    [char] => println!(">> {char} was {action} to the scene."),
                    chars => println!(">> {} were {action} to the scene.", chars.join(", ")),
                }
                event = vm.resume(ResumeEvent::Advance).unwrap();
            }
            _ => (),
        }
    }
    println!("Dialogue ended !")
}

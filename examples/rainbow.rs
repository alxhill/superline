use std::env;
use std::time::Duration;

use superline::modules::*;
use superline::powerline::Separator;
use superline::powerline::{PowerlineLeftBuilder, PowerlineRightBuilder, PowerlineShellBuilder};
use superline::terminal::Shell;
use superline::themes::CustomTheme;

fn main() {
    let args = env::args().collect::<Vec<String>>();

    let (status, duration, columns): (&str, &str, &str) = match args.as_slice() {
        [_, status, duration, columns] => (status, duration, columns),
        [_, status, duration] => (status, duration, "0"),
        _ => ("0", "0", "0"),
    };

    let columns = str::parse::<usize>(columns).unwrap_or(0);
    let duration = str::parse::<u64>(duration).map(Duration::from_millis).ok();

    CustomTheme::load(concat!(env!("CARGO_MANIFEST_DIR"), "/themes/rainbow.json"))
        .expect("the rainbow theme loads");

    superline::Powerline::builder()
        .set_shell(Shell::Bare) // override this to whatever shell you use
        .change_separator(Separator::Chevron)
        .add_module(Spacer::<CustomTheme>::small())
        .add_module(Cwd::<CustomTheme>::new(60, 5, false))
        .add_module(ReadOnly::<CustomTheme>::new())
        .add_module(Spacer::<CustomTheme>::small())
        .add_module(Git::<CustomTheme>::new())
        .start_right()
        .change_separator(Separator::Round)
        .add_module(Python::<CustomTheme>::default())
        .add_padding(0)
        .render(columns);

    superline::Powerline::builder()
        .set_shell(Shell::Bare) // override this to whatever shell you use
        .add_module(LastCmdDuration::<CustomTheme>::new(
            duration,
            Duration::from_millis(0),
        ))
        .add_module(Cmd::<CustomTheme>::new(status))
        .render(columns);
}

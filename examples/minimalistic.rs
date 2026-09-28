use superline::modules::*;
use superline::powerline::{PowerlineRightBuilder, PowerlineShellBuilder};
use superline::terminal::Shell;
use superline::themes::CustomTheme;

fn main() {
    CustomTheme::load(concat!(env!("CARGO_MANIFEST_DIR"), "/themes/simple.json"))
        .expect("the simple theme loads");

    superline::Powerline::builder()
        .set_shell(Shell::Bare) // override this to whatever shell you use
        .add_module(Cwd::<CustomTheme>::new(45, 4, false))
        .add_module(Git::<CustomTheme>::new())
        .add_module(ReadOnly::<CustomTheme>::new())
        .add_module(Cmd::<CustomTheme>::new("0"))
        .render(0);
}

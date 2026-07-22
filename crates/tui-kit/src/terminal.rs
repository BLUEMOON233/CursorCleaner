use std::{io, panic};

use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub type AppTerminal = Terminal<CrosstermBackend<io::Stdout>>;
type PanicHook = Box<dyn Fn(&panic::PanicHookInfo<'_>) + Sync + Send + 'static>;

pub struct TerminalGuard {
    terminal: AppTerminal,
    previous_hook: Option<PanicHook>,
    #[cfg(target_os = "windows")]
    _console_encoding: windows_console::ConsoleEncoding,
}

impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        #[cfg(target_os = "windows")]
        let console_encoding = windows_console::ConsoleEncoding::utf8()?;
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, Hide) {
            let _ = disable_raw_mode();
            return Err(error);
        }

        let previous_hook = panic::take_hook();
        panic::set_hook(Box::new(|info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
            eprintln!("{info}");
        }));

        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self {
                terminal,
                previous_hook: Some(previous_hook),
                #[cfg(target_os = "windows")]
                _console_encoding: console_encoding,
            }),
            Err(error) => {
                let _ = disable_raw_mode();
                let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
                panic::set_hook(previous_hook);
                Err(error)
            }
        }
    }

    pub fn terminal_mut(&mut self) -> &mut AppTerminal {
        &mut self.terminal
    }
}

#[cfg(target_os = "windows")]
mod windows_console {
    use std::io;

    const UTF8_CODE_PAGE: u32 = 65001;

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn GetConsoleCP() -> u32;
        fn GetConsoleOutputCP() -> u32;
        fn SetConsoleCP(code_page: u32) -> i32;
        fn SetConsoleOutputCP(code_page: u32) -> i32;
    }

    pub struct ConsoleEncoding {
        input: u32,
        output: u32,
    }

    impl ConsoleEncoding {
        pub fn utf8() -> io::Result<Self> {
            // SAFETY: These functions do not retain pointers and operate on the current process console.
            let (input, output) = unsafe { (GetConsoleCP(), GetConsoleOutputCP()) };
            if input == 0 || output == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: 65001 is the documented Windows UTF-8 code page identifier.
            if unsafe { SetConsoleOutputCP(UTF8_CODE_PAGE) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: Same process-wide console operation as above.
            if unsafe { SetConsoleCP(UTF8_CODE_PAGE) } == 0 {
                // SAFETY: Restore the value returned by GetConsoleOutputCP.
                let _ = unsafe { SetConsoleOutputCP(output) };
                return Err(io::Error::last_os_error());
            }
            Ok(Self { input, output })
        }
    }

    impl Drop for ConsoleEncoding {
        fn drop(&mut self) {
            // SAFETY: Restore code pages captured from this console at construction time.
            let _ = unsafe { SetConsoleCP(self.input) };
            // SAFETY: Restore code pages captured from this console at construction time.
            let _ = unsafe { SetConsoleOutputCP(self.output) };
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
        if let Some(previous_hook) = self.previous_hook.take() {
            panic::set_hook(previous_hook);
        }
    }
}

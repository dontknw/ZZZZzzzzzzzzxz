//! Terminal: modo "tecla a tecla", leitura de teclas e preparação do console.

pub enum Entrada {
    Enter,
    Cima,
    Baixo,
    Char(char),
    Esc,
    Sair,
    /// Tecla sem função no launcher (ignorada).
    Outra,
}

pub use plat::{ativar, ler_tecla, modo_linha, modo_tecla};

// ============================================================
//  Unix: usa `stty` (sem dependências extras)
// ============================================================
#[cfg(not(target_os = "windows"))]
mod plat {
    use super::Entrada;
    use std::io::{self, Read};
    use std::process::{Command, Stdio};
    use std::sync::OnceLock;

    /// Configuração original do terminal (saída de `stty -g`), se houver tty.
    static ORIGINAL: OnceLock<Option<String>> = OnceLock::new();

    pub struct Guard;

    fn stty(args: &[&str]) {
        let _ = Command::new("stty")
            .args(args)
            .stdin(Stdio::inherit())
            .stderr(Stdio::null())
            .status();
    }

    fn original() -> Option<&'static str> {
        ORIGINAL.get()?.as_deref()
    }

    /// Entra no modo tecla-a-tecla. O terminal volta ao estado original
    /// quando o `Guard` retornado sai de escopo.
    pub fn ativar() -> Guard {
        let salvo = Command::new("stty")
            .arg("-g")
            .stdin(Stdio::inherit())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        let _ = ORIGINAL.set(salvo);
        modo_tecla();
        Guard
    }

    /// Sem Enter, sem eco; Ctrl+C chega como byte 0x03.
    pub fn modo_tecla() {
        if original().is_some() {
            stty(&["-icanon", "-echo", "-isig", "min", "1"]);
        }
    }

    /// Modo normal (linha inteira, com eco) — para digitar a senha.
    pub fn modo_linha() {
        if let Some(salvo) = original() {
            stty(&[salvo]);
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            modo_linha();
        }
    }

    pub fn ler_tecla() -> Entrada {
        let mut buf = [0u8; 16];
        let n = io::stdin().read(&mut buf).unwrap_or(0);
        match &buf[..n] {
            [] => Entrada::Sair,
            [b'\n' | b'\r', ..] => Entrada::Enter,
            [0x1b, b'[' | b'O', b'A', ..] => Entrada::Cima,
            [0x1b, b'[' | b'O', b'B', ..] => Entrada::Baixo,
            [0x1b, ..] => Entrada::Esc,
            [b'q' | b'Q' | 0x03 | 0x04, ..] => Entrada::Sair,
            [c, ..] if c.is_ascii() => Entrada::Char(*c as char),
            _ => Entrada::Outra,
        }
    }
}

// ============================================================
//  Windows: _getch + kernel32 (sem dependências extras)
// ============================================================
#[cfg(target_os = "windows")]
mod plat {
    use super::Entrada;
    use std::ffi::c_void;

    unsafe extern "C" {
        fn _getch() -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(n: u32) -> *mut c_void;
        fn GetConsoleMode(h: *mut c_void, modo: *mut u32) -> i32;
        fn SetConsoleMode(h: *mut c_void, modo: u32) -> i32;
        fn GetConsoleOutputCP() -> u32;
        fn SetConsoleOutputCP(cp: u32) -> i32;
    }

    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    const CP_UTF8: u32 = 65001;

    pub struct Guard {
        code_page: u32,
        modo: Option<u32>,
    }

    /// Habilita UTF-8 e sequências ANSI. O console volta ao estado original
    /// quando o `Guard` retornado sai de escopo (senão o code page ficaria
    /// alterado no terminal depois que o launcher fecha).
    pub fn ativar() -> Guard {
        unsafe {
            let code_page = GetConsoleOutputCP();
            SetConsoleOutputCP(CP_UTF8);

            let h = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut modo = 0u32;
            let modo_original = if GetConsoleMode(h, &mut modo) != 0 {
                SetConsoleMode(h, modo | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
                Some(modo)
            } else {
                None
            };

            Guard {
                code_page,
                modo: modo_original,
            }
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe {
                SetConsoleOutputCP(self.code_page);
                if let Some(modo) = self.modo {
                    SetConsoleMode(GetStdHandle(STD_OUTPUT_HANDLE), modo);
                }
            }
        }
    }

    // No Windows `_getch` já lê tecla a tecla e sem eco; nada a alternar.
    pub fn modo_tecla() {}
    pub fn modo_linha() {}

    pub fn ler_tecla() -> Entrada {
        let c = unsafe { _getch() };
        // 0 ou 0xE0 = prefixo de tecla especial (setas...); o código vem depois.
        if c == 0 || c == 0xE0 {
            return match unsafe { _getch() } {
                72 => Entrada::Cima,
                80 => Entrada::Baixo,
                _ => Entrada::Outra,
            };
        }
        match c {
            c if c < 0 => Entrada::Sair,
            13 | 10 => Entrada::Enter,
            27 => Entrada::Esc,
            3 | 4 | 26 | 113 | 81 => Entrada::Sair, // Ctrl+C/D/Z, q, Q
            1..=127 => Entrada::Char(c as u8 as char),
            _ => Entrada::Outra,
        }
    }
}

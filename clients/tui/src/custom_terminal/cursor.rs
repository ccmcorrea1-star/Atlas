//! Repara o glyph sob comandos de estilo do cursor em terminais JediTerm antigos.
//!
//! Este helper emite comandos de estilo apenas em um glyph proprio, nao ignorado,
//! com largura positiva e dentro da linha. Depois, redesenha o glyph com o mesmo
//! estilo e hyperlink. Sem uma ancora segura, o comando e omitido.

use std::io;
use std::io::Write;

use crossterm::cursor::SetCursorStyle;
use ratatui::backend::Backend;
use ratatui::buffer::CellDiffOption;
use ratatui::buffer::CellWidth;
use ratatui::layout::Position;

use super::DrawCommand;
use super::Terminal;
use super::draw;

impl<B> Terminal<B>
where
    B: Backend<Error = io::Error> + Write,
{
    pub(crate) fn invalidate_cursor_state(&mut self) {
        self.last_cursor_style = None;
        // Um programa externo ou troca de tela pode exibir um cursor que julgavamos oculto.
        self.hidden_cursor = false;
    }

    pub(super) fn set_cursor_style_with_repair(
        &mut self,
        cursor_style: SetCursorStyle,
    ) -> io::Result<()> {
        if self.last_cursor_style == Some(cursor_style) {
            return Ok(());
        }
        // O JediTerm anterior a 3.56 imprime o intermediario de espaco do DECSCUSR no cursor.
        // Aplica um estilo alterado sobre um glyph proprio e o repara mesmo em frames inalterados.
        // https://github.com/JetBrains/jediterm/commit/0c4524f2978bddae65a46c35f264bf89e2ed58fd
        let buffer = &self.buffers[self.current];
        let anchor = (0..buffer.area.height).find_map(|row| {
            let row_start = usize::from(row) * usize::from(buffer.area.width);
            let mut column = 0;
            while column < usize::from(buffer.area.width) {
                let cell = &buffer.content[row_start + column];
                let width = usize::from(cell.cell_width());
                let is_skip = cell.diff_option == CellDiffOption::Skip;
                if !is_skip && width > 0 && column + width <= usize::from(buffer.area.width) {
                    let (x, y) = buffer.pos_of(row_start + column);
                    return Some((Position { x, y }, cell.clone()));
                }
                column += width.max(1);
            }
            None
        });
        // Viewports vazios ou controlados externamente nao possuem cell segura para reparo.
        if let Some((anchor, cell)) = anchor {
            if !self.hidden_cursor {
                self.hide_cursor()?;
            }
            self.set_cursor_position(anchor)?;
            self.set_cursor_style(cursor_style)?;
            let Position { x, y } = anchor;
            draw(
                &mut self.backend,
                std::iter::once(DrawCommand::Put { x, y, cell }),
            )?;
        }

        Ok(())
    }
}

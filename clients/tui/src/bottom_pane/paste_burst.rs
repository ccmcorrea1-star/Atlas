//! Detecta rajadas de paste em terminais sem paste delimitado.
//!
//! Em algumas plataformas, especialmente Windows, pastes chegam como uma sequencia
//! rapida de eventos de tecla em vez de um unico evento de paste.
//!
//! A maquina evita efeitos colaterais durante o paste, trata Enter como newline
//! dentro da entrada e evita flicker ao reclassificar o prefixo.
//!
//! `PasteBurst` e uma maquina de estados pura. O `ChatComposer` fornece eventos
//! que produzem texto e aplica a decisao de digitar ou acumular a entrada:
//!
//! - reter brevemente o primeiro caractere ASCII;
//! - acumular a rajada como um unico paste; ou
//! - encaminhar a entrada como digitacao normal.
//!
//! A maquina retém o primeiro caractere por um intervalo curto, acumula
//! entradas rápidas e descarrega o texto pelo caminho normal de paste.

use std::time::Duration;
use std::time::Instant;

// Limites heuristicos para detectar rajadas de paste.
// Detecta cedo para evitar exibir o prefixo digitado antes do reconhecimento.
const PASTE_BURST_MIN_CHARS: u16 = 3;
const PASTE_ENTER_SUPPRESS_WINDOW: Duration = Duration::from_millis(120);

// Atraso maximo entre caracteres consecutivos para pertencerem a uma rajada.
const PASTE_BURST_CHAR_INTERVAL: Duration = Duration::from_millis(8);

// Tempo ocioso antes de descarregar o conteudo acumulado.
// Rajadas mais lentas foram observadas em ambientes Windows.
#[cfg(not(windows))]
const PASTE_BURST_ACTIVE_IDLE_TIMEOUT: Duration = Duration::from_millis(8);
#[cfg(windows)]
const PASTE_BURST_ACTIVE_IDLE_TIMEOUT: Duration = Duration::from_millis(60);

#[derive(Debug, Default)]
pub(crate) struct PasteBurst {
    last_plain_char_time: Option<Instant>,
    consecutive_plain_char_burst: u16,
    burst_window_until: Option<Instant>,
    buffer: String,
    active: bool,
    // Mantem brevemente o primeiro caractere rapido para evitar flicker.
    pending_first_char: Option<(char, Instant)>,
}

#[allow(dead_code)]
pub(crate) enum CharDecision {
    /// Inicia o buffer e captura retroativamente caracteres ja inseridos.
    BeginBuffer { retro_chars: u16 },
    /// O buffer esta ativo; adiciona o caractere atual.
    BufferAppend,
    /// Ainda nao insere ou renderiza; guarda o primeiro caractere rapido enquanto
    /// verifica se uma rajada de paste sera formada.
    RetainFirstChar,
    /// Inicia o buffer usando o primeiro caractere guardado, sem captura retroativa.
    BeginBufferFromPending,
}

#[allow(dead_code)]
pub(crate) struct RetroGrab {
    pub start_byte: usize,
    pub grabbed: String,
}

pub(crate) enum FlushResult {
    Paste(String),
    Typed(char),
    None,
}

#[allow(dead_code)]
impl PasteBurst {
    /// Atraso recomendado entre teclas simuladas ou antes de um tick para que
    /// uma tecla pendente seja descarregada como digitacao normal.
    ///
    /// Usado principalmente por testes e pela TUI para cruzar o limite de tempo.
    pub fn recommended_flush_delay() -> Duration {
        PASTE_BURST_CHAR_INTERVAL + Duration::from_millis(1)
    }

    pub(crate) fn recommended_active_flush_delay() -> Duration {
        PASTE_BURST_ACTIVE_IDLE_TIMEOUT + Duration::from_millis(1)
    }

    pub(crate) fn is_in_progress(&self) -> bool {
        self.active || !self.buffer.is_empty() || self.pending_first_char.is_some()
    }

    /// Decide como tratar um caractere simples considerando o tempo atual.
    pub fn on_plain_char(&mut self, ch: char, now: Instant) -> CharDecision {
        self.note_plain_char(now);

        if self.active {
            self.burst_window_until = Some(now + PASTE_ENTER_SUPPRESS_WINDOW);
            return CharDecision::BufferAppend;
        }

        // Se o primeiro caractere esta retido e chega outro rapidamente, inicia
        // o buffer sem captura retroativa, pois o primeiro nunca foi renderizado.
        if let Some((held, held_at)) = self.pending_first_char
            && now.duration_since(held_at) <= PASTE_BURST_CHAR_INTERVAL
        {
            self.active = true;
            // O primeiro caractere ja foi capturado no buffer.
            let _ = self.pending_first_char.take();
            self.buffer.push(held);
            self.burst_window_until = Some(now + PASTE_ENTER_SUPPRESS_WINDOW);
            return CharDecision::BeginBufferFromPending;
        }

        if self.consecutive_plain_char_burst >= PASTE_BURST_MIN_CHARS {
            return CharDecision::BeginBuffer {
                retro_chars: self.consecutive_plain_char_burst.saturating_sub(1),
            };
        }

        // Guarda brevemente o primeiro caractere rapido para verificar se a rajada continua.
        self.pending_first_char = Some((ch, now));
        CharDecision::RetainFirstChar
    }

    /// Similar a `on_plain_char()`, mas nunca retem o primeiro caractere.
    ///
    /// Usado para entradas nao ASCII, como IME, em que reter um caractere pode
    /// parecer perda de entrada, mantendo a deteccao de paste por rajada.
    ///
    /// Esta funcao retorna apenas `BufferAppend` ou `BeginBuffer`.
    pub fn on_plain_char_no_hold(&mut self, now: Instant) -> Option<CharDecision> {
        self.note_plain_char(now);

        if self.active {
            self.burst_window_until = Some(now + PASTE_ENTER_SUPPRESS_WINDOW);
            return Some(CharDecision::BufferAppend);
        }

        if self.consecutive_plain_char_burst >= PASTE_BURST_MIN_CHARS {
            return Some(CharDecision::BeginBuffer {
                retro_chars: self.consecutive_plain_char_burst.saturating_sub(1),
            });
        }

        None
    }

    fn note_plain_char(&mut self, now: Instant) {
        match self.last_plain_char_time {
            Some(prev) if now.duration_since(prev) <= PASTE_BURST_CHAR_INTERVAL => {
                self.consecutive_plain_char_burst =
                    self.consecutive_plain_char_burst.saturating_add(1)
            }
            _ => self.consecutive_plain_char_burst = 1,
        }
        self.last_plain_char_time = Some(now);
    }

    /// Descarrega a rajada acumulada quando o tempo entre teclas termina.
    ///
    /// Retorna:
    ///
    /// - [`FlushResult::Paste`] quando a rajada ativa e acumulada vira um unico paste.
    /// - [`FlushResult::Typed`] quando o primeiro caractere rapido foi retido e nenhuma rajada
    ///   surgiu antes do timeout.
    /// - [`FlushResult::None`] quando o timeout nao terminou ou nao ha nada para descarregar.
    pub fn flush_if_due(&mut self, now: Instant) -> FlushResult {
        let timeout = if self.is_active_internal() {
            PASTE_BURST_ACTIVE_IDLE_TIMEOUT
        } else {
            PASTE_BURST_CHAR_INTERVAL
        };
        let timed_out = self
            .last_plain_char_time
            .is_some_and(|t| now.duration_since(t) > timeout);
        if timed_out && self.is_active_internal() {
            self.active = false;
            let out = std::mem::take(&mut self.buffer);
            FlushResult::Paste(out)
        } else if timed_out {
            // Sem uma rajada, descarrega o caractere retido como digitacao normal.
            if let Some((ch, _at)) = self.pending_first_char.take() {
                FlushResult::Typed(ch)
            } else {
                FlushResult::None
            }
        } else {
            FlushResult::None
        }
    }

    /// Acumula newline ou tab na rajada em vez de acionar um atalho.
    /// O primeiro caractere retido entra no buffer antes do caractere de controle.
    ///
    /// Retorna true quando o caractere foi adicionado no contexto de uma rajada.
    pub fn append_control_char_if_active(&mut self, ch: char, now: Instant) -> bool {
        if self.is_active() {
            if let Some((held, _)) = self.pending_first_char.take() {
                self.buffer.push(held);
            }
            self.active = true;
            self.buffer.push(ch);
            self.last_plain_char_time = Some(now);
            self.burst_window_until = Some(now + PASTE_ENTER_SUPPRESS_WINDOW);
            true
        } else {
            false
        }
    }

    /// Decide se Enter insere newline no contexto de rajada ou envia a mensagem.
    pub fn newline_should_insert_instead_of_submit(&self, now: Instant) -> bool {
        let in_burst_window = self.burst_window_until.is_some_and(|until| now <= until);
        self.is_active() || in_burst_window
    }

    /// Decide se Enter insere newline para chamadores que inserem caracteres imediatamente.
    pub fn direct_insert_newline_should_insert(&self, now: Instant) -> bool {
        self.newline_should_insert_instead_of_submit(now)
            || self
                .last_plain_char_time
                .is_some_and(|t| now.duration_since(t) <= PASTE_BURST_CHAR_INTERVAL)
    }

    /// Mantem ativa a janela da rajada.
    pub fn extend_window(&mut self, now: Instant) {
        self.burst_window_until = Some(now + PASTE_ENTER_SUPPRESS_WINDOW);
    }

    /// Inicia o buffer com texto capturado retroativamente.
    pub fn begin_with_retro_grabbed(&mut self, grabbed: String, now: Instant) {
        if !grabbed.is_empty() {
            self.buffer.push_str(&grabbed);
        }
        self.active = true;
        self.burst_window_until = Some(now + PASTE_ENTER_SUPPRESS_WINDOW);
    }

    /// Adiciona um caractere ao buffer da rajada.
    pub fn append_char_to_buffer(&mut self, ch: char, now: Instant) {
        self.buffer.push(ch);
        self.burst_window_until = Some(now + PASTE_ENTER_SUPPRESS_WINDOW);
    }

    /// Tenta adicionar um caractere apenas se uma rajada ja estiver ativa.
    /// Retorna true quando o caractere foi capturado na rajada existente.
    pub fn try_append_char_if_active(&mut self, ch: char, now: Instant) -> bool {
        if self.active || !self.buffer.is_empty() {
            self.append_char_to_buffer(ch, now);
            true
        } else {
            false
        }
    }

    /// Decide se deve iniciar o buffer capturando caracteres recentes antes do cursor.
    ///
    /// Se o trecho capturado possui espaco ou pelo menos 16 caracteres, trata-o
    /// como paste para evitar flicker em URLs, caminhos e textos multilinha.
    /// Retorna `Some(RetroGrab)` quando decide capturar retroativamente; caso contrario, `None`.
    pub fn decide_begin_buffer(
        &mut self,
        now: Instant,
        before: &str,
        retro_chars: usize,
    ) -> Option<RetroGrab> {
        let start_byte = retro_start_index(before, retro_chars);
        let grabbed = before[start_byte..].to_string();
        let looks_pastey =
            grabbed.chars().any(char::is_whitespace) || grabbed.chars().count() >= 16;
        if looks_pastey {
            // O chamador remove este trecho do texto da UI.
            self.begin_with_retro_grabbed(grabbed.clone(), now);
            Some(RetroGrab {
                start_byte,
                grabbed,
            })
        } else {
            None
        }
    }

    /// Descarrega a rajada antes de aplicar entrada modificada ou nao textual.
    pub fn flush_before_modified_input(&mut self) -> Option<String> {
        if !self.is_active() {
            return None;
        }
        self.active = false;
        let mut out = std::mem::take(&mut self.buffer);
        if let Some((ch, _at)) = self.pending_first_char.take() {
            out.push(ch);
        }
        Some(out)
    }

    /// Limpa apenas a janela de tempo e o primeiro caractere pendente.
    /// Nao emite nem limpa o texto acumulado; o chamador deve descarrega-lo antes.
    pub fn clear_window_after_non_char(&mut self) {
        self.consecutive_plain_char_burst = 0;
        self.last_plain_char_time = None;
        self.burst_window_until = None;
        self.active = false;
        self.pending_first_char = None;
    }

    /// Retorna true durante qualquer estado transitorio relacionado a uma rajada.
    pub fn is_active(&self) -> bool {
        self.is_active_internal() || self.pending_first_char.is_some()
    }

    fn is_active_internal(&self) -> bool {
        self.active || !self.buffer.is_empty()
    }

    pub fn clear_after_explicit_paste(&mut self) {
        self.last_plain_char_time = None;
        self.consecutive_plain_char_burst = 0;
        self.burst_window_until = None;
        self.active = false;
        self.buffer.clear();
        self.pending_first_char = None;
    }
}

pub(crate) fn retro_start_index(before: &str, retro_chars: usize) -> usize {
    if retro_chars == 0 {
        return before.len();
    }
    before
        .char_indices()
        .rev()
        .nth(retro_chars.saturating_sub(1))
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Comportamento: retencao breve do primeiro caractere ASCII sem formar uma rajada.
    #[test]
    fn ascii_first_char_is_held_then_flushes_as_typed() {
        let mut burst = PasteBurst::default();
        let t0 = Instant::now();
        assert!(matches!(
            burst.on_plain_char('a', t0),
            CharDecision::RetainFirstChar
        ));

        let t1 = t0 + PasteBurst::recommended_flush_delay() + Duration::from_millis(1);
        assert!(matches!(burst.flush_if_due(t1), FlushResult::Typed('a')));
        assert!(!burst.is_active());
    }

    /// Comportamento: dois caracteres ASCII rapidos formam uma rajada sem renderizar o primeiro.
    #[test]
    fn ascii_two_fast_chars_start_buffer_from_pending_and_flush_as_paste() {
        let mut burst = PasteBurst::default();
        let t0 = Instant::now();
        assert!(matches!(
            burst.on_plain_char('a', t0),
            CharDecision::RetainFirstChar
        ));

        let t1 = t0 + Duration::from_millis(1);
        assert!(matches!(
            burst.on_plain_char('b', t1),
            CharDecision::BeginBufferFromPending
        ));
        burst.append_char_to_buffer('b', t1);

        let t2 = t1 + PasteBurst::recommended_active_flush_delay() + Duration::from_millis(1);
        assert!(matches!(
            burst.flush_if_due(t2),
            FlushResult::Paste(ref s) if s == "ab"
        ));
    }

    /// Comportamento: entrada nao textual descarrega o estado transitorio antes de ser aplicada.
    #[test]
    fn flush_before_modified_input_includes_pending_first_char() {
        let mut burst = PasteBurst::default();
        let t0 = Instant::now();
        assert!(matches!(
            burst.on_plain_char('a', t0),
            CharDecision::RetainFirstChar
        ));

        assert_eq!(burst.flush_before_modified_input(), Some("a".to_string()));
        assert!(!burst.is_active());
    }

    /// Comportamento: captura retroativa exige um prefixo parecido com paste para evitar
    /// classificar rajadas curtas de IME incorretamente.
    #[test]
    fn decide_begin_buffer_only_triggers_for_pastey_prefixes() {
        let mut burst = PasteBurst::default();
        let now = Instant::now();

        assert!(
            burst
                .decide_begin_buffer(now, "ab", /*retro_chars*/ 2)
                .is_none()
        );
        assert!(!burst.is_active());

        let grab = burst
            .decide_begin_buffer(now, "a b", /*retro_chars*/ 2)
            .expect("whitespace should be considered paste-like");
        assert_eq!(grab.start_byte, 1);
        assert_eq!(grab.grabbed, " b");
        assert!(burst.is_active());
    }

    /// Comportamento: apos uma rajada, Enter continua inserindo newline durante uma janela curta.
    #[test]
    fn newline_suppression_window_outlives_buffer_flush() {
        let mut burst = PasteBurst::default();
        let t0 = Instant::now();
        assert!(matches!(
            burst.on_plain_char('a', t0),
            CharDecision::RetainFirstChar
        ));

        let t1 = t0 + Duration::from_millis(1);
        assert!(matches!(
            burst.on_plain_char('b', t1),
            CharDecision::BeginBufferFromPending
        ));
        burst.append_char_to_buffer('b', t1);

        let t2 = t1 + PasteBurst::recommended_active_flush_delay() + Duration::from_millis(1);
        assert!(matches!(burst.flush_if_due(t2), FlushResult::Paste(ref s) if s == "ab"));
        assert!(!burst.is_active());

        assert!(burst.newline_should_insert_instead_of_submit(t2));
        let t3 = t1 + PASTE_ENTER_SUPPRESS_WINDOW + Duration::from_millis(1);
        assert!(!burst.newline_should_insert_instead_of_submit(t3));
    }
}

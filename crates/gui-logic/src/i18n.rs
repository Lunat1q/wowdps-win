//! The GUI's words in another language: English is the key, and a word the
//! table lacks reads in English. The window's and the overlay's text sinks
//! (`W::text`, `Ov::words`, the tooltips) pass every line through [`tr`],
//! so the code that words a line keeps writing English and its tests keep
//! reading it.
//!
//! A line that holds a figure or a name ("Raid 3", "Pinned Durgan. …")
//! matches a template whose `{}` stand for any text, in order.

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::OnceLock;

/// A language the GUI speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    En,
    Ru,
    Es,
    De,
}

impl Lang {
    pub const ALL: [Lang; 4] = [Lang::En, Lang::Ru, Lang::Es, Lang::De];

    /// Its config key (`language = "ru"`).
    pub fn key(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
            Lang::Es => "es",
            Lang::De => "de",
        }
    }

    /// A config key read; anything unknown is English.
    pub fn from_key(key: &str) -> Lang {
        Lang::ALL
            .into_iter()
            .find(|l| l.key().eq_ignore_ascii_case(key.trim()))
            .unwrap_or_default()
    }

    /// The game client's locales whose data speaks it, in preference
    /// order: what `gen-talent-trees --locale` names a dataset by.
    pub fn game_locales(self) -> &'static [&'static str] {
        match self {
            Lang::En => &[],
            Lang::Ru => &["ruRU"],
            Lang::Es => &["esES", "esMX"],
            Lang::De => &["deDE"],
        }
    }

    /// Its name in itself, for the selector (never translated).
    pub fn native(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ru => "Русский",
            Lang::Es => "Español",
            Lang::De => "Deutsch",
        }
    }
}

// ponytail: per thread — GPUI draws every line on its main thread, and each
// test (a thread of its own) keeps its own language.
thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::En) };
}

pub fn set(lang: Lang) {
    CURRENT.with(|c| c.set(lang));
}

pub fn current() -> Lang {
    CURRENT.with(Cell::get)
}

/// `text` in the current language.
pub fn tr(text: &str) -> Cow<'_, str> {
    tr_in(current(), text)
}

/// `text` in `lang`: the exact line's entry, else the first template that
/// fits it, else `text` as it came.
pub fn tr_in(lang: Lang, text: &str) -> Cow<'_, str> {
    let col = match lang {
        Lang::En => return Cow::Borrowed(text),
        Lang::Ru => 0,
        Lang::Es => 1,
        Lang::De => 2,
    };
    let pick = |e: &Entry| match col {
        0 => e.1,
        1 => e.2,
        _ => e.3,
    };
    let (exact, templates) = index();
    if let Some(e) = exact.get(text) {
        return Cow::Borrowed(pick(e));
    }
    // ponytail: a linear scan of ~60 templates per line drawn; a cache
    // keyed by the line if a frame ever shows it.
    for e in templates {
        if let Some(holes) = fit(e.0, text) {
            return Cow::Owned(fill(pick(e), &holes));
        }
    }
    Cow::Borrowed(text)
}

/// A compiled-in word in the current language, still `'static`.
pub fn tr_static(text: &'static str) -> &'static str {
    match tr(text) {
        Cow::Borrowed(b) => b,
        Cow::Owned(_) => text,
    }
}

/// A day of a month as the language writes it: "Nov 22", "22 ноя",
/// "22 de noviembre", "22. November". `month` is already translated.
pub fn month_day(month: &str, day: u32, long: bool) -> String {
    match current() {
        Lang::En => format!("{month} {day}"),
        Lang::Ru => format!("{day} {month}"),
        Lang::Es if long => format!("{day} de {month}"),
        Lang::Es => format!("{day} {month}"),
        Lang::De => format!("{day}. {month}"),
    }
}

/// "Subtlety Rogue"; elsewhere the class first, its spec after it.
pub fn spec_class(spec: &str, class: &str) -> String {
    match current() {
        Lang::En => format!("{spec} {class}"),
        _ => format!("{} ({})", tr(class), tr(spec)),
    }
}

/// "1st"; "1-й", "1.º", "1.".
pub fn ordinal(n: usize) -> String {
    match current() {
        Lang::En => {
            let suffix = match (n % 10, n % 100) {
                (_, 11..=13) => "th",
                (1, _) => "st",
                (2, _) => "nd",
                (3, _) => "rd",
                _ => "th",
            };
            format!("{n}{suffix}")
        }
        Lang::Ru => format!("{n}-й"),
        Lang::Es => format!("{n}.º"),
        Lang::De => format!("{n}."),
    }
}

/// The holes of `template` (its `{}`) as `text` fills them, if it fits.
fn fit<'t>(template: &str, text: &'t str) -> Option<Vec<&'t str>> {
    let mut parts = template.split("{}");
    let head = parts.next()?;
    let mut rest = text.strip_prefix(head)?;
    let mut holes = Vec::new();
    let parts: Vec<&str> = parts.collect();
    for (i, part) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        let at = if last {
            if !rest.ends_with(part) {
                return None;
            }
            rest.len().checked_sub(part.len())?
        } else {
            rest.find(part)?
        };
        let (hole, after) = rest.split_at_checked(at)?;
        if hole.is_empty() {
            return None;
        }
        holes.push(hole);
        rest = after.get(part.len()..)?;
    }
    rest.is_empty().then_some(holes)
}

fn fill(template: &str, holes: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut holes = holes.iter();
    let mut parts = template.split("{}").peekable();
    while let Some(part) = parts.next() {
        out.push_str(part);
        if parts.peek().is_some() {
            out.push_str(holes.next().copied().unwrap_or_default());
        }
    }
    out
}

type Entry = (&'static str, &'static str, &'static str, &'static str);

fn index() -> &'static (HashMap<&'static str, &'static Entry>, Vec<&'static Entry>) {
    static INDEX: OnceLock<(HashMap<&'static str, &'static Entry>, Vec<&'static Entry>)> =
        OnceLock::new();
    INDEX.get_or_init(|| {
        let (templates, exact): (Vec<&Entry>, Vec<&Entry>) =
            WORDS.iter().partition(|e| e.0.contains("{}"));
        let mut templates = templates;
        // The most literal text first, so "died {} times" beats "died {}".
        templates.sort_by_key(|e| std::cmp::Reverse(e.0.replace("{}", "").len()));
        (exact.into_iter().map(|e| (e.0, e)).collect(), templates)
    })
}

/// English, Russian, Spanish, German.
#[rustfmt::skip]
const WORDS: &[Entry] = &[
    // Views and the bar.
    ("Damage", "Урон", "Daño", "Schaden"),
    ("Healing", "Исцеление", "Sanación", "Heilung"),
    ("Taken", "Получено", "Recibido", "Erlitten"),
    ("Deaths", "Смерти", "Muertes", "Tode"),
    ("Interrupts", "Прерывания", "Interrupciones", "Unterbrechungen"),
    ("Crowd control", "Контроль", "Control de masas", "Kontrolle"),
    ("Dispels", "Рассеивания", "Disipaciones", "Bannungen"),
    ("Enemies", "Враги", "Enemigos", "Gegner"),
    ("DAMAGE", "УРОН", "DAÑO", "SCHADEN"),
    ("HEALING", "ИСЦЕЛЕНИЕ", "SANACIÓN", "HEILUNG"),
    ("TAKEN", "ПОЛУЧЕНО", "RECIBIDO", "ERLITTEN"),
    ("DEATHS", "СМЕРТИ", "MUERTES", "TODE"),
    ("INTERRUPTS", "ПРЕРЫВАНИЯ", "INTERRUPCIONES", "UNTERBRECHUNGEN"),
    ("DISPELS", "РАССЕИВАНИЯ", "DISIPACIONES", "BANNUNGEN"),
    ("ENEMIES", "ВРАГИ", "ENEMIGOS", "GEGNER"),
    ("Options", "Настройки", "Opciones", "Optionen"),
    ("Row ranks", "Номера строк", "Rangos de fila", "Zeilenränge"),
    ("Hide realm names", "Скрыть названия миров", "Ocultar reinos", "Realmnamen ausblenden"),
    ("Theme", "Тема", "Tema", "Design"),
    ("Chrome", "Оформление", "Interfaz", "Rahmen"),
    ("Your class", "Ваш класс", "Tu clase", "Deine Klasse"),
    ("Language", "Язык", "Idioma", "Sprache"),
    ("Home", "Главная", "Inicio", "Start"),
    ("Live", "Сейчас", "En vivo", "Live"),
    ("Latest", "Последний", "Último", "Neuester"),
    ("Fights", "Бои", "Combates", "Kämpfe"),
    ("Filter", "Фильтр", "Filtro", "Filter"),
    ("filter players", "фильтр игроков", "filtrar jugadores", "Spieler filtern"),
    ("All characters", "Все персонажи", "Todos los personajes", "Alle Charaktere"),
    ("Trash", "Трэш", "Trash", "Trash"),
    ("Raid", "Рейд", "Banda", "Schlachtzug"),
    ("Dungeons", "Подземелья", "Mazmorras", "Dungeons"),
    ("Arena", "Арена", "Arena", "Arena"),
    ("Delve", "Вылазка", "Profundidad", "Tiefe"),
    ("Open world", "Открытый мир", "Mundo abierto", "Offene Welt"),
    ("Encounters", "Боссы", "Encuentros", "Begegnungen"),
    ("Boss", "Босс", "Jefe", "Boss"),
    ("Instance", "Подземелье", "Instancia", "Instanz"),
    ("Whole visit", "Весь заход", "Visita completa", "Gesamter Besuch"),
    ("Keyboard", "Клавиатура", "Teclado", "Tastatur"),
    ("Palette", "Палитра", "Paleta", "Palette"),
    ("Meter", "Измеритель", "Medidor", "Meter"),
    ("In combat", "В бою", "En combate", "Im Kampf"),
    // Outcomes.
    ("Kill", "Убит", "Derrotado", "Besiegt"),
    ("Wipe", "Вайп", "Wipe", "Wipe"),
    ("Win", "Победа", "Victoria", "Sieg"),
    ("Loss", "Поражение", "Derrota", "Niederlage"),
    ("Timed", "В срок", "A tiempo", "In Zeit"),
    ("Died", "Умер", "Murió", "Gestorben"),
    ("KILL", "УБИТ", "DERROTADO", "BESIEGT"),
    ("WIPE", "ВАЙП", "WIPE", "WIPE"),
    ("WIN", "ПОБЕДА", "VICTORIA", "SIEG"),
    ("LOSS", "ПОРАЖЕНИЕ", "DERROTA", "NIEDERLAGE"),
    ("TIMED", "В СРОК", "A TIEMPO", "IN ZEIT"),
    ("OVER", "ПРОСРОЧЕН", "FUERA DE TIEMPO", "ÜBERZOGEN"),
    ("LIVE", "СЕЙЧАС", "EN VIVO", "LIVE"),
    ("TIMED {}", "В СРОК {}", "A TIEMPO {}", "IN ZEIT {}"),
    ("OVER {}", "ПРОСРОЧЕН {}", "FUERA DE TIEMPO {}", "ÜBERZOGEN {}"),
    ("LIVE {}", "СЕЙЧАС {}", "EN VIVO {}", "LIVE {}"),
    ("Timed {}", "В срок {}", "A tiempo {}", "In Zeit {}"),
    ("Live {}", "Сейчас {}", "En vivo {}", "Live {}"),
    ("No kill", "Нет убийства", "Sin victoria", "Kein Kill"),
    ("Wipe at {}%", "Вайп на {}%", "Wipe al {}%", "Wipe bei {}%"),
    ("Best {}%", "Лучший {}%", "Mejor {}%", "Bestwert {}%"),
    ("Killed in {}", "Убит за {}", "Derrotado en {}", "Besiegt in {}"),
    ("Raid {}", "Рейд {}", "Banda {}", "Schlachtzug {}"),
    ("Run {}", "Забег {}", "Carrera {}", "Lauf {}"),
    ("Build {}", "Сборка {}", "Versión {}", "Build {}"),
    // Columns and the inspector.
    ("Amount", "Объём", "Cantidad", "Menge"),
    ("Count", "Кол-во", "Recuento", "Anzahl"),
    ("Overheal", "Избыт. исцеление", "Sobrecuración", "Überheilung"),
    ("Absorbed", "Поглощено", "Absorbido", "Absorbiert"),
    ("Per sec", "В сек", "Por seg", "Pro Sek."),
    ("Share", "Доля", "Parte", "Anteil"),
    ("Crit", "Крит", "Crít", "Krit"),
    ("Hits", "Удары", "Golpes", "Treffer"),
    ("Avg", "Сред.", "Prom.", "Ø"),
    ("Total", "Всего", "Total", "Gesamt"),
    ("Time", "Время", "Tiempo", "Zeit"),
    ("Rank", "Место", "Rango", "Rang"),
    ("Ratio", "Соотношение", "Proporción", "Verhältnis"),
    ("Player", "Игрок", "Jugador", "Spieler"),
    ("Players", "Игроки", "Jugadores", "Spieler"),
    ("Ability", "Способность", "Habilidad", "Fähigkeit"),
    ("Abilities", "Способности", "Habilidades", "Fähigkeiten"),
    ("Target", "Цель", "Objetivo", "Ziel"),
    ("Targets", "Цели", "Objetivos", "Ziele"),
    ("Attacker", "Атакующий", "Atacante", "Angreifer"),
    ("Attackers", "Атакующие", "Atacantes", "Angreifer"),
    ("By target", "По целям", "Por objetivo", "Nach Ziel"),
    ("By ability", "По способностям", "Por habilidad", "Nach Fähigkeit"),
    ("Overkill", "Избыточный урон", "Sobremuerte", "Overkill"),
    ("Killing blow", "Смертельный удар", "Golpe mortal", "Todesstoß"),
    ("Hit by", "Удар от", "Golpeado por", "Getroffen von"),
    ("hit by", "удар от", "golpeado por", "getroffen von"),
    ("Health after", "Здоровье после", "Salud después", "Gesundheit danach"),
    ("Heal absorbed", "Поглощено исцеления", "Sanación absorbida", "Heilung absorbiert"),
    ("Damage taken", "Полученный урон", "Daño recibido", "Erlittener Schaden"),
    ("Damage to enemies", "Урон по врагам", "Daño a enemigos", "Schaden an Gegnern"),
    ("Stacks", "Стаки", "Acumulaciones", "Stapel"),
    ("Change", "Изменение", "Cambio", "Änderung"),
    ("Uptime", "Время действия", "Tiempo activo", "Aktivzeit"),
    ("First", "Первый", "Primero", "Erste"),
    ("Retained", "Сохранено", "Retenido", "Behalten"),
    ("Reduced", "Снижено", "Reducido", "Reduziert"),
    ("Prevented", "Предотвращено", "Prevenido", "Verhindert"),
    ("Mitigated", "Смягчено", "Mitigado", "Gemindert"),
    ("Blocked", "Заблокировано", "Bloqueado", "Geblockt"),
    ("Misses", "Промахи", "Fallos", "Verfehlt"),
    ("Miss", "Промах", "Fallo", "Verfehlt"),
    ("Hit", "Попадание", "Golpe", "Treffer"),
    ("Casts", "Применения", "Lanzamientos", "Zauber"),
    ("Avg cast", "Сред. применение", "Lanzamiento prom.", "Ø Zauber"),
    ("Instant", "Мгновенно", "Instantáneo", "Sofort"),
    ("Channeled", "Поддерживаемое", "Canalizado", "Kanalisiert"),
    ("Passive", "Пассивная", "Pasiva", "Passiv"),
    ("Empowered", "Усиленное", "Potenciado", "Ermächtigt"),
    ("Melee range", "Ближний бой", "Cuerpo a cuerpo", "Nahkampf"),
    ("Unlimited range", "Неограниченная дальность", "Alcance ilimitado", "Unbegrenzte Reichweite"),
    ("Cooldowns", "Кулдауны", "Reutilizaciones", "Abklingzeiten"),
    ("Defensives", "Защитные", "Defensivas", "Defensive"),
    ("Externals", "Внешние", "Externos", "Externe"),
    ("Battle rezzes", "Боевые воскрешения", "Resurrecciones en combate", "Kampfwiederbelebungen"),
    ("Trinket", "Аксессуар", "Abalorio", "Schmuckstück"),
    ("Potion", "Зелье", "Poción", "Trank"),
    ("Other", "Другое", "Otro", "Sonstiges"),
    ("Items", "Предметы", "Objetos", "Gegenstände"),
    ("Physical", "Физический", "Físico", "Körperlich"),
    ("Holy", "Свет", "Sagrado", "Heilig"),
    ("Fire", "Огонь", "Fuego", "Feuer"),
    ("Nature", "Природа", "Naturaleza", "Natur"),
    ("Frost", "Лёд", "Escarcha", "Frost"),
    ("Shadow", "Тьма", "Sombras", "Schatten"),
    ("Arcane", "Тайная магия", "Arcano", "Arkan"),
    ("Per second", "В секунду", "Por segundo", "Pro Sekunde"),
    ("Cumulative", "Накопительно", "Acumulado", "Kumulativ"),
    ("Over time", "Со временем", "En el tiempo", "Im Verlauf"),
    ("Recap", "Сводка", "Resumen", "Zusammenfassung"),
    ("Death recap", "Сводка смерти", "Resumen de muerte", "Todeszusammenfassung"),
    ("Death recap (K)", "Сводка смерти (K)", "Resumen de muerte (K)", "Todeszusammenfassung (K)"),
    ("Talents and gear", "Таланты и экипировка", "Talentos y equipo", "Talente und Ausrüstung"),
    ("Talents and gear (t)", "Таланты и экипировка (t)", "Talentos y equipo (t)", "Talente und Ausrüstung (t)"),
    ("Their damage", "Их урон", "Su daño", "Ihr Schaden"),
    ("Their damage (d)", "Их урон (d)", "Su daño (d)", "Ihr Schaden (d)"),
    ("Compare", "Сравнить", "Comparar", "Vergleichen"),
    ("Stop comparing", "Прекратить сравнение", "Dejar de comparar", "Vergleich beenden"),
    ("Stop comparing (v)", "Прекратить сравнение (v)", "Dejar de comparar (v)", "Vergleich beenden (v)"),
    ("Stop comparing (v, Esc)", "Прекратить сравнение (v, Esc)", "Dejar de comparar (v, Esc)", "Vergleich beenden (v, Esc)"),
    ("Pin for comparison (v)", "Закрепить для сравнения (v)", "Fijar para comparar (v)", "Zum Vergleich anheften (v)"),
    ("Pick another player to compare.", "Выберите другого игрока для сравнения.", "Elige otro jugador para comparar.", "Wähle einen weiteren Spieler zum Vergleich."),
    ("Pinned, pick another", "Закреплено, выберите другого", "Fijado, elige otro", "Angeheftet, wähle einen weiteren"),
    ("Pinned {}. Move to another player to compare.", "Закреплён {}. Перейдите к другому игроку для сравнения.", "{} fijado. Pasa a otro jugador para comparar.", "{} angeheftet. Wechsle zu einem anderen Spieler zum Vergleich."),
    ("One scale, one time axis. Move to swap the second player, v to stop.", "Одна шкала, одна ось времени. Перейдите, чтобы сменить второго игрока, v — стоп.", "Una escala, un eje de tiempo. Muévete para cambiar el segundo jugador, v para parar.", "Eine Skala, eine Zeitachse. Wechseln tauscht den zweiten Spieler, v beendet."),
    ("Pinned: retention keeps it (p)", "Закреплено: пулл будет сохранён (p)", "Fijado: se conservará (p)", "Angeheftet: bleibt erhalten (p)"),
    ("Pinned: retention keeps this pull", "Закреплено: пулл будет сохранён", "Fijado: se conservará este pull", "Angeheftet: dieser Pull bleibt erhalten"),
    ("Unpinned: retention may remove this pull", "Откреплено: пулл может быть удалён", "Sin fijar: este pull puede eliminarse", "Gelöst: dieser Pull kann entfernt werden"),
    ("Go to the live pull (m)", "К текущему пуллу (m)", "Ir al pull en curso (m)", "Zum laufenden Pull (m)"),
    ("Go to the latest pull (m)", "К последнему пуллу (m)", "Ir al último pull (m)", "Zum neuesten Pull (m)"),
    ("Back to the meter (Esc)", "Назад к измерителю (Esc)", "Volver al medidor (Esc)", "Zurück zum Meter (Esc)"),
    ("Back to the list (Esc)", "Назад к списку (Esc)", "Volver a la lista (Esc)", "Zurück zur Liste (Esc)"),
    ("Pulls (Esc closes)", "Пуллы (Esc закрывает)", "Pulls (Esc cierra)", "Pulls (Esc schließt)"),
    ("Jump to a pull, player or view", "Перейти к пуллу, игроку или виду", "Saltar a un pull, jugador o vista", "Zu Pull, Spieler oder Ansicht springen"),
    ("Per second or cumulative (g)", "В секунду или накопительно (g)", "Por segundo o acumulado (g)", "Pro Sekunde oder kumulativ (g)"),
    ("Stack the graph, or draw the total alone", "Сложить график или показать только итог", "Apilar el gráfico o dibujar solo el total", "Graph stapeln oder nur die Summe zeigen"),
    ("Last events, oldest first", "Последние события, сначала старые", "Últimos eventos, primero los antiguos", "Letzte Ereignisse, älteste zuerst"),
    ("Enemy, every unit with this name folded together", "Враг: все существа с этим именем вместе", "Enemigo: todas las unidades con este nombre juntas", "Gegner: alle Einheiten dieses Namens zusammengefasst"),
    ("Select your row", "Выбрать свою строку", "Selecciona tu fila", "Deine Zeile wählen"),
    ("Select a player to inspect them here.", "Выберите игрока, чтобы изучить его здесь.", "Selecciona un jugador para inspeccionarlo aquí.", "Wähle einen Spieler, um ihn hier zu untersuchen."),
    ("Select an enemy to see who hit it.", "Выберите врага, чтобы увидеть, кто его бил.", "Selecciona un enemigo para ver quién lo golpeó.", "Wähle einen Gegner, um zu sehen, wer ihn traf."),
    ("Select a death to see its recap.", "Выберите смерть, чтобы увидеть сводку.", "Selecciona una muerte para ver su resumen.", "Wähle einen Tod, um seine Zusammenfassung zu sehen."),
    ("Nobody died in this pull.", "В этом пулле никто не умер.", "Nadie murió en este pull.", "In diesem Pull ist niemand gestorben."),
    ("Nothing to inspect yet.", "Пока нечего изучать.", "Nada que inspeccionar aún.", "Noch nichts zu untersuchen."),
    ("Nothing yet", "Пока ничего", "Nada aún", "Noch nichts"),
    ("Nothing matches. Try a boss, a player or a view.", "Ничего не найдено. Попробуйте босса, игрока или вид.", "Nada coincide. Prueba un jefe, un jugador o una vista.", "Keine Treffer. Versuche einen Boss, einen Spieler oder eine Ansicht."),
    ("Waiting for this view's numbers…", "Ожидание данных этого вида…", "Esperando los datos de esta vista…", "Warte auf die Zahlen dieser Ansicht…"),
    ("Waiting for this enemy's numbers…", "Ожидание данных этого врага…", "Esperando los datos de este enemigo…", "Warte auf die Zahlen dieses Gegners…"),
    ("Waiting for the recap…", "Ожидание сводки…", "Esperando el resumen…", "Warte auf die Zusammenfassung…"),
    ("waiting for combat…", "ожидание боя…", "esperando combate…", "warte auf Kampf…"),
    ("reading the stored pull…", "чтение сохранённого пулла…", "leyendo el pull guardado…", "lese gespeicherten Pull…"),
    ("loading comparison…", "загрузка сравнения…", "cargando comparación…", "lade Vergleich…"),
    ("Loading the comparison…", "Загрузка сравнения…", "Cargando la comparación…", "Lade den Vergleich…"),
    ("No damage logged", "Урон не записан", "No se registró daño", "Kein Schaden protokolliert"),
    ("no damage recorded", "урон не записан", "sin daño registrado", "kein Schaden erfasst"),
    ("no healing recorded", "исцеление не записано", "sin sanación registrada", "keine Heilung erfasst"),
    ("no deaths", "нет смертей", "sin muertes", "keine Tode"),
    ("nothing landed", "ничего не попало", "nada impactó", "nichts getroffen"),
    ("nothing interrupted", "ничего не прервано", "nada interrumpido", "nichts unterbrochen"),
    ("nothing dispelled", "ничего не рассеяно", "nada disipado", "nichts gebannt"),
    ("nothing controlled", "никого не контролировали", "nada controlado", "nichts kontrolliert"),
    ("pick two players to compare", "выберите двух игроков для сравнения", "elige dos jugadores para comparar", "wähle zwei Spieler zum Vergleich"),
    ("not in the history store", "нет в хранилище истории", "no está en el historial", "nicht im Verlauf"),
    ("enemy team", "команда противника", "equipo enemigo", "gegnerisches Team"),
    ("this character", "этот персонаж", "este personaje", "dieser Charakter"),
    ("someone else", "кто-то другой", "otra persona", "jemand anderes"),
    ("personal best", "личный рекорд", "mejor marca personal", "persönliche Bestleistung"),
    ("effective dps", "эффективный DPS", "DPS efectivo", "effektive DPS"),
    ("the paste carried no talent strings", "во вставке нет строк талантов", "lo pegado no contenía cadenas de talentos", "die Einfügung enthielt keine Talent-Strings"),
    ("died {} times, first {}", "умер {} раз, впервые {}", "murió {} veces, la primera {}", "starb {}-mal, zuerst {}"),
    ("died {} times", "умер {} раз", "murió {} veces", "starb {}-mal"),
    ("died {}", "умер {}", "murió {}", "starb {}"),
    ("rezzed {}", "воскрешён {}", "resucitado {}", "wiederbelebt {}"),
    ("alive {}", "жив {}", "vivo {}", "lebt {}"),
    ("best {}", "лучший {}", "mejor {}", "bester {}"),
    ("at {}%", "на {}%", "al {}%", "bei {}%"),
    ("of {}", "из {}", "de {}", "von {}"),
    ("per second, {} s", "в секунду, {} с", "por segundo, {} s", "pro Sekunde, {} s"),
    ("no events for {}s", "нет событий {} с", "sin eventos durante {} s", "keine Ereignisse für {} s"),
    // History store.
    ("No stored card for this pull yet: the store writes one when it ends", "Для этого пулла ещё нет карточки: она создаётся по его окончании", "Aún no hay ficha para este pull: se guarda al terminar", "Noch keine Karte für diesen Pull: sie wird bei seinem Ende geschrieben"),
    ("The history store kept this pull's rows, not this player's breakdown.", "Хранилище истории сохранило строки пулла, но не разбивку этого игрока.", "El historial guardó las filas del pull, no el desglose de este jugador.", "Der Verlauf hat die Zeilen des Pulls behalten, nicht die Aufschlüsselung dieses Spielers."),
    ("The history store kept this pull's rows, not what a comparison needs", "Хранилище истории сохранило строки пулла, но не данные для сравнения", "El historial guardó las filas del pull, no lo que necesita una comparación", "Der Verlauf hat die Zeilen des Pulls behalten, nicht, was ein Vergleich braucht"),
    ("The history store keeps no enemy damage", "Хранилище истории не хранит урон врагов", "El historial no guarda el daño enemigo", "Der Verlauf speichert keinen Gegnerschaden"),
    ("The history store keeps an ability's own curve for kills, timed keys and pinned pulls", "Хранилище истории хранит кривую способности для убийств, ключей в срок и закреплённых пуллов", "El historial guarda la curva de una habilidad para victorias, llaves a tiempo y pulls fijados", "Der Verlauf speichert die Kurve einer Fähigkeit für Kills, rechtzeitige Schlüssel und angeheftete Pulls"),
    ("Comparing needs the pull's details: the store kept its rows alone", "Для сравнения нужны детали пулла: хранилище сохранило только строки", "Comparar necesita los detalles del pull: solo se guardaron sus filas", "Vergleichen braucht die Details des Pulls: gespeichert sind nur seine Zeilen"),
    ("The history store is off: {}.", "Хранилище истории выключено: {}.", "El historial está desactivado: {}.", "Der Verlauf ist aus: {}."),
    ("Only the newest {} pulls of the week were read.", "Прочитаны только {} последних пуллов недели.", "Solo se leyeron los {} pulls más recientes de la semana.", "Nur die neuesten {} Pulls der Woche wurden gelesen."),
    ("The daemon dropped {}: this is not the whole week.", "Демон отбросил {}: это не вся неделя.", "El daemon descartó {}: esta no es la semana completa.", "Der Daemon hat {} verworfen: das ist nicht die ganze Woche."),
    // Home.
    ("Your week", "Ваша неделя", "Tu semana", "Deine Woche"),
    ("Your week is on its way from the history store.", "Ваша неделя загружается из хранилища истории.", "Tu semana está llegando del historial.", "Deine Woche wird aus dem Verlauf geladen."),
    ("Your rank, pull by pull", "Ваше место, пулл за пуллом", "Tu rango, pull a pull", "Dein Rang, Pull für Pull"),
    ("You, this week", "Вы, на этой неделе", "Tú, esta semana", "Du, diese Woche"),
    ("You", "Вы", "Tú", "Du"),
    ("Tonight", "Сегодня", "Esta noche", "Heute Abend"),
    ("Tonight you played", "Сегодня вы играли", "Esta noche jugaste", "Heute Abend gespielt"),
    ("Last night you played", "Вчера вы играли", "Anoche jugaste", "Gestern Abend gespielt"),
    ("Playing now", "Сейчас в игре", "Jugando ahora", "Spielt gerade"),
    ("Keys this week", "Ключи на этой неделе", "Llaves esta semana", "Schlüssel diese Woche"),
    ("Mythic+ keys", "Ключи М+", "Llaves de M+", "M+-Schlüssel"),
    ("Effective dps on keys", "Эффективный DPS в ключах", "DPS efectivo en llaves", "Effektive DPS in Schlüsseln"),
    ("Follow the character I'm playing", "Следить за текущим персонажем", "Seguir al personaje que juego", "Dem gespielten Charakter folgen"),
    ("The window follows whichever of your characters is in the pull.", "Окно следует за тем из ваших персонажей, кто в пулле.", "La ventana sigue al personaje tuyo que esté en el pull.", "Das Fenster folgt dem deiner Charaktere, der im Pull ist."),
    ("No pulls stored this week.", "За эту неделю пуллов не сохранено.", "No hay pulls guardados esta semana.", "Diese Woche keine Pulls gespeichert."),
    ("No pulls on {} this week.", "Нет пуллов на {} за эту неделю.", "Sin pulls en {} esta semana.", "Diese Woche keine Pulls auf {}."),
    ("No {} fights yet", "Пока нет боёв {}", "Aún no hay combates de {}", "Noch keine {}-Kämpfe"),
    ("Play a fight in {} with combat logging on and it shows up here.", "Проведите бой в {} с включённым журналом боя, и он появится здесь.", "Juega un combate en {} con el registro de combate activado y aparecerá aquí.", "Spiele einen Kampf in {} mit aktivem Kampflog, dann erscheint er hier."),
    ("No pulls yet. Pulls appear here as you fight, and earlier nights load from your history.", "Пуллов пока нет. Они появляются здесь во время боя, а прошлые вечера загружаются из истории.", "Aún no hay pulls. Aparecen aquí mientras combates, y las noches anteriores se cargan del historial.", "Noch keine Pulls. Sie erscheinen hier, während du kämpfst; frühere Abende werden aus dem Verlauf geladen."),
    ("Rank is among your role, by {}.", "Место среди вашей роли, по {}.", "El rango es dentro de tu rol, por {}.", "Rang innerhalb deiner Rolle, nach {}."),
    ("one dot per pull", "одна точка на пулл", "un punto por pull", "ein Punkt pro Pull"),
    ("each dot a run", "каждая точка — забег", "cada punto una carrera", "jeder Punkt ein Lauf"),
    ("top of the role", "лучший в роли", "lo mejor del rol", "Spitze der Rolle"),
    ("raid pulls", "рейдовые пуллы", "pulls de banda", "Schlachtzug-Pulls"),
    ("keys played as dps", "ключи за DPS", "llaves jugadas como DPS", "als DPS gespielte Schlüssel"),
    ("kill or timed", "убийство или в срок", "victoria o a tiempo", "Kill oder in Zeit"),
    ("wipe or over time", "вайп или просрочено", "wipe o fuera de tiempo", "Wipe oder überzogen"),
    ("run time against the timer", "время забега против таймера", "tiempo de carrera contra el temporizador", "Laufzeit gegen den Timer"),
    ("played tonight", "сыграно сегодня", "jugado esta noche", "heute Abend gespielt"),
    // The rail, the bar, the fight head.
    ("Pulls", "Пуллы", "Pulls", "Pulls"),
    ("Hide trash", "Скрыть трэш", "Ocultar trash", "Trash ausblenden"),
    ("Filter players", "Фильтр игроков", "Filtrar jugadores", "Spieler filtern"),
    ("Jump to a pull, player or view (Ctrl K)", "Перейти к пуллу, игроку или виду (Ctrl K)", "Saltar a un pull, jugador o vista (Ctrl K)", "Zu Pull, Spieler oder Ansicht springen (Ctrl K)"),
    ("paste an in-game talent string…", "вставьте строку талантов из игры…", "pega una cadena de talentos del juego…", "Talent-String aus dem Spiel einfügen…"),
    ("Raid dps", "Рейд DPS", "DPS de banda", "Schlachtzug-DPS"),
    ("Run dps", "DPS забега", "DPS de carrera", "Lauf-DPS"),
    ("Raid dps, peak {}", "Рейд DPS, пик {}", "DPS de banda, pico {}", "Schlachtzug-DPS, Spitze {}"),
    ("Run dps, peak {}", "DPS забега, пик {}", "DPS de carrera, pico {}", "Lauf-DPS, Spitze {}"),
    ("Follower", "Соратники", "Seguidores", "Anhänger"),
    ("{}, Follower", "{}, соратники", "{}, seguidores", "{}, Anhänger"),
    ("you", "вы", "tú", "du"),
    ("alive {}, rezzed by {}", "жив {}, воскресил {}", "vivo {}, resucitado por {}", "lebt {}, wiederbelebt von {}"),
    ("alive {}, rezzed", "жив {}, воскрешён", "vivo {}, resucitado", "lebt {}, wiederbelebt"),
    ("{} gained", "{} получено", "{} ganado", "{} gewonnen"),
    ("{} wasted", "{} потеряно", "{} desperdiciado", "{} verschwendet"),
    // Resources.
    ("Energy", "Энергия", "Energía", "Energie"),
    ("Combo points", "Приёмы серии", "Puntos de combo", "Combopunkte"),
    ("Mana", "Мана", "Maná", "Mana"),
    ("Rage", "Ярость", "Ira", "Wut"),
    ("Focus", "Концентрация", "Enfoque", "Fokus"),
    ("Runic power", "Сила рун", "Poder rúnico", "Runenmacht"),
    ("Runes", "Руны", "Runas", "Runen"),
    ("Soul shards", "Осколки души", "Fragmentos de alma", "Seelensplitter"),
    ("Holy power", "Энергия Света", "Poder sagrado", "Heilige Kraft"),
    ("Maelstrom", "Энергия Водоворота", "Vorágine", "Mahlstrom"),
    ("Chi", "Ци", "Chi", "Chi"),
    ("Insanity", "Безумие", "Locura", "Wahnsinn"),
    ("Fury", "Неистовство", "Furia", "Furor"),
    ("Pain", "Боль", "Dolor", "Schmerz"),
    ("Essence", "Сущность", "Esencia", "Essenz"),
    ("Astral power", "Астральная сила", "Poder astral", "Astrale Macht"),
    ("Lunar power", "Астральная сила", "Poder astral", "Astrale Macht"),
    ("Arcane charges", "Заряды тайной магии", "Cargas arcanas", "Arkanladungen"),
    // Classes and specs (the game's own names).
    ("Warrior", "Воин", "Guerrero", "Krieger"),
    ("Paladin", "Паладин", "Paladín", "Paladin"),
    ("Hunter", "Охотник", "Cazador", "Jäger"),
    ("Rogue", "Разбойник", "Pícaro", "Schurke"),
    ("Priest", "Жрец", "Sacerdote", "Priester"),
    ("Death Knight", "Рыцарь смерти", "Caballero de la Muerte", "Todesritter"),
    ("Shaman", "Шаман", "Chamán", "Schamane"),
    ("Mage", "Маг", "Mago", "Magier"),
    ("Warlock", "Чернокнижник", "Brujo", "Hexenmeister"),
    ("Monk", "Монах", "Monje", "Mönch"),
    ("Druid", "Друид", "Druida", "Druide"),
    ("Demon Hunter", "Охотник на демонов", "Cazador de demonios", "Dämonenjäger"),
    ("Evoker", "Пробудитель", "Evocador", "Rufer"),
    ("Arms", "Оружие", "Armas", "Waffen"),
    ("Protection", "Защита", "Protección", "Schutz"),
    ("Retribution", "Воздаяние", "Reprensión", "Vergeltung"),
    ("Beast Mastery", "Повелитель зверей", "Bestias", "Tierherrschaft"),
    ("Marksmanship", "Стрельба", "Puntería", "Treffsicherheit"),
    ("Survival", "Выживание", "Supervivencia", "Überleben"),
    ("Assassination", "Ликвидация", "Asesinato", "Meucheln"),
    ("Outlaw", "Головорез", "Forajido", "Gesetzlosigkeit"),
    ("Subtlety", "Скрытность", "Sutileza", "Täuschung"),
    ("Discipline", "Послушание", "Disciplina", "Disziplin"),
    ("Blood", "Кровь", "Sangre", "Blut"),
    ("Unholy", "Нечестивость", "Profano", "Unheilig"),
    ("Elemental", "Стихии", "Elemental", "Elementar"),
    ("Enhancement", "Совершенствование", "Mejora", "Verstärkung"),
    ("Restoration", "Исцеление", "Restauración", "Wiederherstellung"),
    ("Affliction", "Колдовство", "Aflicción", "Gebrechen"),
    ("Demonology", "Демонология", "Demonología", "Dämonologie"),
    ("Destruction", "Разрушение", "Destrucción", "Zerstörung"),
    ("Brewmaster", "Хмелевар", "Maestro cervecero", "Braumeister"),
    ("Mistweaver", "Ткач туманов", "Tejedor de niebla", "Nebelwirker"),
    ("Windwalker", "Танцующий с ветром", "Viajero del viento", "Windläufer"),
    ("Balance", "Баланс", "Equilibrio", "Gleichgewicht"),
    ("Feral", "Сила зверя", "Feral", "Wildheit"),
    ("Guardian", "Страж", "Guardián", "Wächter"),
    ("Havoc", "Истребление", "Devastación", "Verwüstung"),
    ("Vengeance", "Месть", "Venganza", "Rachsucht"),
    ("Devourer", "Пожиратель", "Devorador", "Verschlinger"),
    ("Devastation", "Опустошение", "Devastación", "Verheerung"),
    ("Preservation", "Сохранение", "Preservación", "Bewahrung"),
    ("Augmentation", "Насыщение", "Aumento", "Verstärkung"),
    // Dates: full months as a date writes them, then their short forms.
    ("January", "января", "enero", "Januar"),
    ("February", "февраля", "febrero", "Februar"),
    ("March", "марта", "marzo", "März"),
    ("April", "апреля", "abril", "April"),
    ("May", "мая", "mayo", "Mai"),
    ("June", "июня", "junio", "Juni"),
    ("July", "июля", "julio", "Juli"),
    ("August", "августа", "agosto", "August"),
    ("September", "сентября", "septiembre", "September"),
    ("October", "октября", "octubre", "Oktober"),
    ("November", "ноября", "noviembre", "November"),
    ("December", "декабря", "diciembre", "Dezember"),
    ("Jan", "янв", "ene", "Jan."),
    ("Feb", "фев", "feb", "Feb."),
    ("Mar", "мар", "mar", "März"),
    ("Apr", "апр", "abr", "Apr."),
    ("Jun", "июн", "jun", "Juni"),
    ("Jul", "июл", "jul", "Juli"),
    ("Aug", "авг", "ago", "Aug."),
    ("Sep", "сен", "sept", "Sep."),
    ("Oct", "окт", "oct", "Okt."),
    ("Nov", "ноя", "nov", "Nov."),
    ("Dec", "дек", "dic", "Dez."),
    ("Monday", "Понедельник", "Lunes", "Montag"),
    ("Tuesday", "Вторник", "Martes", "Dienstag"),
    ("Wednesday", "Среда", "Miércoles", "Mittwoch"),
    ("Thursday", "Четверг", "Jueves", "Donnerstag"),
    ("Friday", "Пятница", "Viernes", "Freitag"),
    ("Saturday", "Суббота", "Sábado", "Samstag"),
    ("Sunday", "Воскресенье", "Domingo", "Sonntag"),
    ("Mon", "Пн", "Lun", "Mo"),
    ("Tue", "Вт", "Mar", "Di"),
    ("Wed", "Ср", "Mié", "Mi"),
    ("Thu", "Чт", "Jue", "Do"),
    ("Fri", "Пт", "Vie", "Fr"),
    ("Sat", "Сб", "Sáb", "Sa"),
    ("Sun", "Вс", "Dom", "So"),
    // The keys sheet.
    ("What works on {}. Any key or click closes this.", "Что работает в «{}». Любая клавиша или щелчок закроет это.", "Qué funciona en {}. Cualquier tecla o clic cierra esto.", "Was in {} funktioniert. Jede Taste oder jeder Klick schließt das."),
    ("move", "перемещение", "mover", "bewegen"),
    ("views", "виды", "vistas", "Ansichten"),
    ("inspector", "инспектор", "inspector", "Inspektor"),
    ("go to", "перейти", "ir a", "gehe zu"),
    ("back one level", "на уровень назад", "volver un nivel", "eine Ebene zurück"),
    ("damage", "урон", "daño", "Schaden"),
    ("healing", "исцеление", "sanación", "Heilung"),
    ("taken", "получено", "recibido", "erlitten"),
    ("deaths", "смерти", "muertes", "Tode"),
    ("interrupts", "прерывания", "interrupciones", "Unterbrechungen"),
    ("crowd control", "контроль", "control de masas", "Kontrolle"),
    ("dispels", "рассеивания", "disipaciones", "Bannungen"),
    ("enemies", "враги", "enemigos", "Gegner"),
    ("talents and gear", "таланты и экипировка", "talentos y equipo", "Talente und Ausrüstung"),
    ("pin the pull, or let it go", "закрепить пулл или открепить", "fijar el pull o soltarlo", "Pull anheften oder lösen"),
    ("jump to anything", "перейти куда угодно", "saltar a cualquier cosa", "zu allem springen"),
    ("live pull", "текущий пулл", "pull en curso", "laufender Pull"),
    ("home", "главная", "inicio", "Start"),
    ("earlier nights", "прошлые вечера", "noches anteriores", "frühere Abende"),
    ("this sheet", "эта справка", "esta hoja", "diese Übersicht"),
    ("zoom in", "увеличить", "acercar", "vergrößern"),
    ("zoom out", "уменьшить", "alejar", "verkleinern"),
    ("reset zoom", "сбросить масштаб", "restablecer zoom", "Zoom zurücksetzen"),
    ("quit", "выход", "salir", "beenden"),
    ("next or previous player", "следующий или предыдущий игрок", "jugador siguiente o anterior", "nächster oder vorheriger Spieler"),
    ("older or newer pull", "более старый или новый пулл", "pull anterior o posterior", "älterer oder neuerer Pull"),
    ("open or inspect", "открыть или изучить", "abrir o inspeccionar", "öffnen oder untersuchen"),
    ("fold or unfold an ability", "свернуть или развернуть способность", "plegar o desplegar una habilidad", "Fähigkeit ein- oder ausklappen"),
    ("widen the inspector, or narrow it", "расширить или сузить инспектор", "ensanchar o estrechar el inspector", "Inspektor verbreitern oder verschmälern"),
    ("pin for comparison, or stop", "закрепить для сравнения или прекратить", "fijar para comparar o parar", "zum Vergleich anheften oder beenden"),
    ("per second or cumulative", "в секунду или накопительно", "por segundo o acumulado", "pro Sekunde oder kumulativ"),
    ("abilities or targets", "способности или цели", "habilidades u objetivos", "Fähigkeiten oder Ziele"),
    ("deaths: each in turn", "смерти: по очереди", "muertes: una a una", "Tode: der Reihe nach"),
    ("pull, or recap's deaths", "пулл или смерти сводки", "pull, o muertes del resumen", "Pull oder Tode der Zusammenfassung"),
    ("an ability's drill", "разбор способности", "desglose de una habilidad", "Aufschlüsselung einer Fähigkeit"),
    ("the meter", "измеритель", "el medidor", "das Meter"),
    ("the pull list", "список пуллов", "la lista de pulls", "die Pull-Liste"),
    ("the comparison", "сравнение", "la comparación", "der Vergleich"),
    ("the talent viewer", "просмотр талантов", "el visor de talentos", "die Talentansicht"),
    // The talent viewer: its chrome, the inventory, the tooltip's own words
    // (a talent's name and text are the game data's, in the client's language).
    ("talents", "таланты", "talentos", "Talente"),
    ("inventory", "инвентарь", "inventario", "Inventar"),
    ("paste simc/string", "вставить simc/строку", "pegar simc/cadena", "simc/String einfügen"),
    ("loadouts", "наборы", "configuraciones", "Loadouts"),
    ("active", "активный", "activa", "aktiv"),
    ("dataset build {}", "данные сборки {}", "datos de la versión {}", "Datensatz-Build {}"),
    ("from combat log", "из журнала боя", "del registro de combate", "aus dem Kampflog"),
    ("edited", "изменено", "editado", "bearbeitet"),
    ("copy string", "копировать строку", "copiar cadena", "String kopieren"),
    ("copied ✓", "скопировано ✓", "copiado ✓", "kopiert ✓"),
    ("paste a talent string or a SimulationCraft export to see a build", "вставьте строку талантов или экспорт SimulationCraft, чтобы увидеть билд", "pega una cadena de talentos o una exportación de SimulationCraft para ver una build", "Talent-String oder SimulationCraft-Export einfügen, um einen Build zu sehen"),
    ("click picks (+1 rank) · right-click refunds · octagons open their option picker · ctrl +/− text size · esc closes · tab flips inventory · drop a simc export here", "клик выбирает (+1 ранг) · правый клик возвращает · восьмиугольники открывают выбор · ctrl +/− размер текста · esc закрывает · tab — инвентарь · перетащите сюда экспорт simc", "clic elige (+1 rango) · clic derecho devuelve · los octágonos abren su selector · ctrl +/− tamaño del texto · esc cierra · tab cambia al inventario · suelta aquí una exportación de simc", "Klick wählt (+1 Rang) · Rechtsklick erstattet · Achtecke öffnen ihre Auswahl · Strg +/− Textgröße · Esc schließt · Tab wechselt zum Inventar · simc-Export hier ablegen"),
    ("cannot read {}", "не удаётся прочитать {}", "no se puede leer {}", "{} kann nicht gelesen werden"),
    ("equipped", "надето", "equipado", "angelegt"),
    ("in bags", "в сумках", "en las bolsas", "in Taschen"),
    ("currencies", "валюта", "monedas", "Währungen"),
    ("equipped — from combat log", "надето — из журнала боя", "equipado — del registro de combate", "angelegt — aus dem Kampflog"),
    ("item {}", "предмет {}", "objeto {}", "Gegenstand {}"),
    ("the clipboard is empty", "буфер обмена пуст", "el portapapeles está vacío", "die Zwischenablage ist leer"),
    ("Talent", "Талант", "Talento", "Talent"),
    ("Rank {}/{}", "Ранг {}/{}", "Rango {}/{}", "Rang {}/{}"),
    ("Rank {}", "Ранг {}", "Rango {}", "Rang {}"),
    ("Requires {}", "Требуется: {}", "Requiere {}", "Benötigt {}"),
    ("{}/{} pts", "{}/{} очк.", "{}/{} ptos.", "{}/{} Pkt."),
    ("{} pts", "{} очк.", "{} ptos.", "{} Pkt."),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_the_key_and_unknown_words_pass_through() {
        assert_eq!(tr_in(Lang::En, "Damage"), "Damage");
        assert_eq!(tr_in(Lang::Ru, "Damage"), "Урон");
        assert_eq!(tr_in(Lang::De, "Fel Firebolt"), "Fel Firebolt");
    }

    #[test]
    fn templates_carry_their_holes() {
        assert_eq!(tr_in(Lang::Es, "Raid 3"), "Banda 3");
        assert_eq!(tr_in(Lang::De, "Wipe at 12%"), "Wipe bei 12%");
        assert_eq!(
            tr_in(Lang::Ru, "died 3 times, first 1:02"),
            "умер 3 раз, впервые 1:02"
        );
        assert_eq!(
            tr_in(Lang::Es, "Pinned Ada. Move to another player to compare."),
            "Ada fijado. Pasa a otro jugador para comparar."
        );
        // A hole is never empty, and the whole line must fit.
        assert_eq!(tr_in(Lang::Ru, "Raid "), "Raid ");
        assert_eq!(tr_in(Lang::Ru, "Raids"), "Raids");
        // The talent tooltip's rank, and the meter's longer line still its own.
        assert_eq!(tr_in(Lang::Ru, "Rank 2/3"), "Ранг 2/3");
        assert_eq!(
            tr_in(Lang::De, "Rank is among your role, by dps."),
            "Rang innerhalb deiner Rolle, nach dps."
        );
    }

    #[test]
    fn every_entry_has_every_language_and_its_holes() {
        for e in WORDS {
            let holes = e.0.matches("{}").count();
            for t in [e.1, e.2, e.3] {
                assert!(!t.is_empty(), "{}", e.0);
                assert_eq!(t.matches("{}").count(), holes, "{} → {t}", e.0);
            }
        }
        let mut keys: Vec<&str> = WORDS.iter().map(|e| e.0).collect();
        keys.sort_unstable();
        let len = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), len, "a key appears twice");
    }

    #[test]
    fn dates_ordinals_and_specs_follow_the_language() {
        set(Lang::Ru);
        assert_eq!(crate::rail::night_short(20_779, 20_779 + 1), "Вс, 22 ноя");
        assert_eq!(ordinal(1), "1-й");
        assert_eq!(spec_class("Subtlety", "Rogue"), "Разбойник (Скрытность)");
        set(Lang::En);
        assert_eq!(crate::rail::night_short(20_779, 20_779 + 1), "Sun, Nov 22");
        assert_eq!(ordinal(22), "22nd");
    }

    #[test]
    fn config_keys_round_trip() {
        for l in Lang::ALL {
            assert_eq!(Lang::from_key(l.key()), l);
        }
        assert_eq!(Lang::from_key("klingon"), Lang::En);
    }
}

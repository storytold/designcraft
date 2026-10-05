//! Interface language (Edit › Interface Language): translations of menu titles, the most used
//! menu items and panel names. Untranslated strings stay English. The translations are our own.

/// Supported interface languages: (code, name in that language).
pub const LANGUAGES: &[(&str, &str)] = &[("", "English"), ("de", "Deutsch"), ("fr", "Français"), ("es", "Español"), ("ja", "日本語")];

/// English → [German, French, Spanish, Japanese].
const TABLE: &[(&str, [&str; 4])] = &[
    // Menus.
    ("File", ["Datei", "Fichier", "Archivo", "ファイル"]),
    ("Edit", ["Bearbeiten", "Édition", "Edición", "編集"]),
    ("Layout", ["Layout", "Page", "Maquetación", "レイアウト"]),
    ("Type", ["Schrift", "Texte", "Texto", "書式"]),
    ("Object", ["Objekt", "Objet", "Objeto", "オブジェクト"]),
    ("Table", ["Tabelle", "Tableau", "Tabla", "表"]),
    ("View", ["Ansicht", "Affichage", "Ver", "表示"]),
    ("Window", ["Fenster", "Fenêtre", "Ventana", "ウィンドウ"]),
    ("Help", ["Hilfe", "Aide", "Ayuda", "ヘルプ"]),
    // File.
    ("New", ["Neu", "Nouveau", "Nuevo", "新規"]),
    ("New Document…", ["Neues Dokument …", "Nouveau document…", "Nuevo documento…", "新規ドキュメント…"]),
    ("Open…", ["Öffnen …", "Ouvrir…", "Abrir…", "開く…"]),
    ("Close", ["Schließen", "Fermer", "Cerrar", "閉じる"]),
    ("Save", ["Speichern", "Enregistrer", "Guardar", "保存"]),
    ("Save As…", ["Speichern unter …", "Enregistrer sous…", "Guardar como…", "別名で保存…"]),
    ("Save a Copy…", ["Kopie speichern …", "Enregistrer une copie…", "Guardar una copia…", "コピーを保存…"]),
    ("Revert", ["Zurück zur letzten Version", "Version précédente", "Volver a la versión guardada", "復帰"]),
    ("Place…", ["Platzieren …", "Importer…", "Colocar…", "配置…"]),
    ("Export PDF…", ["PDF exportieren …", "Exporter en PDF…", "Exportar PDF…", "PDF を書き出し…"]),
    ("Print…", ["Drucken …", "Imprimer…", "Imprimir…", "プリント…"]),
    ("Document Setup…", ["Dokument einrichten …", "Format de document…", "Ajustar documento…", "ドキュメント設定…"]),
    ("Package…", ["Verpacken …", "Assembler…", "Empaquetar…", "パッケージ…"]),
    // Edit.
    ("Undo", ["Rückgängig", "Annuler", "Deshacer", "取り消し"]),
    ("Redo", ["Wiederholen", "Rétablir", "Rehacer", "やり直し"]),
    ("Cut", ["Ausschneiden", "Couper", "Cortar", "カット"]),
    ("Copy", ["Kopieren", "Copier", "Copiar", "コピー"]),
    ("Paste", ["Einfügen", "Coller", "Pegar", "ペースト"]),
    ("Clear", ["Löschen", "Effacer", "Borrar", "消去"]),
    ("Duplicate", ["Duplizieren", "Dupliquer", "Duplicar", "複製"]),
    ("Select All", ["Alles auswählen", "Tout sélectionner", "Seleccionar todo", "すべてを選択"]),
    ("Deselect All", ["Auswahl aufheben", "Tout désélectionner", "Deseleccionar todo", "すべての選択を解除"]),
    ("Find/Change…", ["Suchen/Ersetzen …", "Rechercher/Remplacer…", "Buscar/Cambiar…", "検索と置換…"]),
    ("Check Spelling…", ["Rechtschreibprüfung …", "Vérifier l'orthographe…", "Revisar ortografía…", "スペルチェック…"]),
    ("Preferences…", ["Voreinstellungen …", "Préférences…", "Preferencias…", "環境設定…"]),
    ("Keyboard Shortcuts…", ["Tastaturbefehle …", "Raccourcis clavier…", "Métodos abreviados de teclado…", "キーボードショートカット…"]),
    ("Interface Language", ["Sprache der Oberfläche", "Langue de l'interface", "Idioma de la interfaz", "インターフェイスの言語"]),
    // Layout.
    ("Pages", ["Seiten", "Pages", "Páginas", "ページ"]),
    ("Margins and Columns…", ["Ränder und Spalten …", "Marges et colonnes…", "Márgenes y columnas…", "マージン・段組…"]),
    ("Create Guides…", ["Hilfslinien erstellen …", "Créer des repères…", "Crear guías…", "ガイドを作成…"]),
    (
        "Numbering & Section Options…",
        [
            "Nummerierungs- und Abschnittsoptionen …",
            "Options de numérotation et de section…",
            "Opciones de numeración y sección…",
            "ノンブルとセクション設定…",
        ],
    ),
    // Type.
    ("Story Direction", ["Textrichtung", "Sens du texte", "Dirección del texto", "組み方向"]),
    ("Horizontal", ["Horizontal", "Horizontal", "Horizontal", "横書き"]),
    ("Vertical", ["Vertikal", "Vertical", "Vertical", "縦書き"]),
    ("Tate-Chu-Yoko", ["Tate-Chu-Yoko", "Tate-Chu-Yoko", "Tate-Chu-Yoko", "縦中横"]),
    ("Ruby…", ["Ruby …", "Ruby…", "Ruby…", "ルビ…"]),
    ("Kenten", ["Kenten", "Kenten", "Kenten", "圏点"]),
    ("Font", ["Schriftart", "Police", "Fuente", "フォント"]),
    ("Size", ["Schriftgrad", "Corps", "Tamaño", "サイズ"]),
    ("Character", ["Zeichen", "Caractère", "Carácter", "文字"]),
    ("Paragraph", ["Absatz", "Paragraphe", "Párrafo", "段落"]),
    ("Tabs", ["Tabulatoren", "Tabulations", "Tabulaciones", "タブ"]),
    ("Glyphs", ["Glyphen", "Glyphes", "Glifos", "字形"]),
    ("Story", ["Textabschnitt", "Article", "Artículo", "ストーリー"]),
    ("Character Styles", ["Zeichenformate", "Styles de caractère", "Estilos de carácter", "文字スタイル"]),
    ("Paragraph Styles", ["Absatzformate", "Styles de paragraphe", "Estilos de párrafo", "段落スタイル"]),
    ("Create Outlines", ["In Pfade umwandeln", "Vectoriser", "Crear contornos", "アウトラインを作成"]),
    ("Show Hidden Characters", ["Verborgene Zeichen einblenden", "Afficher les caractères masqués", "Mostrar caracteres ocultos", "制御文字を表示"]),
    // Object.
    ("Transform", ["Transformieren", "Transformation", "Transformar", "変形"]),
    ("Arrange", ["Anordnen", "Disposition", "Organizar", "重ね順"]),
    ("Group", ["Gruppieren", "Grouper", "Agrupar", "グループ"]),
    ("Ungroup", ["Gruppierung aufheben", "Dissocier", "Desagrupar", "グループ解除"]),
    ("Lock", ["Sperren", "Verrouiller", "Bloquear", "ロック"]),
    ("Hide", ["Ausblenden", "Masquer", "Ocultar", "隠す"]),
    ("Effects", ["Effekte", "Effets", "Efectos", "効果"]),
    ("Fitting", ["Anpassen", "Ajustement", "Encaje", "オブジェクトサイズの調整"]),
    ("Text Frame Options…", ["Textrahmenoptionen …", "Options de bloc de texte…", "Opciones de marco de texto…", "テキストフレーム設定…"]),
    // Table.
    ("Insert Table…", ["Tabelle einfügen …", "Insérer un tableau…", "Insertar tabla…", "表を挿入…"]),
    ("Merge Cells", ["Zellen verbinden", "Fusionner les cellules", "Combinar celdas", "セルを結合"]),
    ("Unmerge Cells", ["Zellverbindung aufheben", "Annuler la fusion des cellules", "Separar celdas", "セルの結合解除"]),
    ("Insert", ["Einfügen", "Insérer", "Insertar", "挿入"]),
    ("Delete", ["Löschen", "Supprimer", "Eliminar", "削除"]),
    // View.
    ("Zoom In", ["Einzoomen", "Zoom avant", "Acercar", "ズームイン"]),
    ("Zoom Out", ["Auszoomen", "Zoom arrière", "Alejar", "ズームアウト"]),
    ("Fit Page in Window", ["Seite in Fenster einpassen", "Page entière", "Encajar página en ventana", "ページ全体"]),
    ("Fit Spread in Window", ["Druckbogen in Fenster einpassen", "Planche entière", "Encajar pliego en ventana", "スプレッド全体"]),
    ("Actual Size", ["Originalgröße", "Taille réelle", "Tamaño real", "100%"]),
    ("Screen Mode", ["Bildschirmmodus", "Mode d'affichage", "Modo de pantalla", "スクリーンモード"]),
    ("Grids & Guides", ["Raster und Hilfslinien", "Grilles et repères", "Cuadrículas y guías", "グリッドとガイド"]),
    ("Show Rulers", ["Lineale einblenden", "Afficher les règles", "Mostrar reglas", "定規を表示"]),
    // Window and panels.
    ("Properties", ["Eigenschaften", "Propriétés", "Propiedades", "プロパティ"]),
    ("Layers", ["Ebenen", "Calques", "Capas", "レイヤー"]),
    ("Links", ["Verknüpfungen", "Liens", "Vínculos", "リンク"]),
    ("Swatches", ["Farbfelder", "Nuancier", "Muestras", "スウォッチ"]),
    ("Color", ["Farbe", "Couleur", "Color", "カラー"]),
    ("Stroke", ["Kontur", "Contour", "Trazo", "線"]),
    ("Gradient", ["Verlauf", "Dégradé", "Degradado", "グラデーション"]),
    ("Align", ["Ausrichten", "Alignement", "Alinear", "整列"]),
    ("Pathfinder", ["Pathfinder", "Pathfinder", "Buscatrazos", "パスファインダー"]),
    ("Info", ["Info", "Informations", "Información", "情報"]),
    ("Library", ["Bibliothek", "Bibliothèque", "Biblioteca", "ライブラリ"]),
    ("Book", ["Buch", "Livre", "Libro", "ブック"]),
    ("Index", ["Index", "Index", "Índice", "索引"]),
    ("Hyperlinks", ["Hyperlinks", "Hyperliens", "Hipervínculos", "ハイパーリンク"]),
    ("Bookmarks", ["Lesezeichen", "Signets", "Marcadores", "ブックマーク"]),
    ("Articles", ["Artikel", "Articles", "Artículos", "アーティクル"]),
    ("Tags", ["Tags", "Balises", "Etiquetas", "タグ"]),
    ("Media", ["Medien", "Média", "Medios", "メディア"]),
    ("Object States", ["Objektstatus", "États d'objet", "Estados de objeto", "オブジェクトステート"]),
    ("Buttons and Forms", ["Schaltflächen und Formulare", "Boutons et formulaires", "Botones y formularios", "ボタンとフォーム"]),
    ("Liquid Layout", ["Flüssiges Layout", "Mise en page liquide", "Maquetación líquida", "リキッドレイアウト"]),
    ("Workspace", ["Arbeitsbereich", "Espace de travail", "Espacio de trabajo", "ワークスペース"]),
    ("Split Window", ["Fenster teilen", "Fractionner la fenêtre", "Dividir ventana", "ウィンドウを分割"]),
    ("New Window", ["Neues Fenster", "Nouvelle fenêtre", "Nueva ventana", "新規ウィンドウ"]),
    (
        "Show All Menu Items",
        ["Alle Menüelemente anzeigen", "Afficher toutes les commandes", "Mostrar todos los elementos de menú", "すべてのメニュー項目を表示"],
    ),
];

fn column(lang: &str) -> Option<usize> {
    match lang {
        "de" => Some(0),
        "fr" => Some(1),
        "es" => Some(2),
        "ja" => Some(3),
        _ => None,
    }
}

/// `s` in `lang` (English, or the string itself, when there's no translation).
pub fn tr<'a>(lang: &str, s: &'a str) -> std::borrow::Cow<'a, str> {
    let Some(c) = column(lang) else { return s.into() };
    match TABLE.iter().find(|(en, _)| *en == s) {
        Some((_, t)) => t[c].to_string().into(),
        None => s.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_known_strings_and_keeps_the_rest() {
        assert_eq!(tr("de", "File"), "Datei");
        assert_eq!(tr("ja", "Swatches"), "スウォッチ");
        assert_eq!(tr("fr", "Something new"), "Something new");
        assert_eq!(tr("", "File"), "File");
        // Every row is unique and complete.
        for (i, (en, t)) in TABLE.iter().enumerate() {
            assert!(TABLE[..i].iter().all(|(e, _)| e != en), "duplicate {en}");
            assert!(t.iter().all(|x| !x.is_empty()), "{en}");
        }
    }

    #[test]
    fn japanese_vertical_type_commands_are_translated() {
        for (en, ja) in [
            ("Story Direction", "組み方向"),
            ("Horizontal", "横書き"),
            ("Vertical", "縦書き"),
            ("Tate-Chu-Yoko", "縦中横"),
            ("Ruby…", "ルビ…"),
            ("Kenten", "圏点"),
        ] {
            assert_eq!(tr("ja", en), ja);
        }
    }
}

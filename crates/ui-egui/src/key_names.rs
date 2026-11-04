/// Named `KeyboardEvent.key` values (W3C UI Events) that a browser reports for a key that types
/// nothing, separated by spaces. eframe's web backend sends the ones it doesn't know as text.
const KEY_NAMES: &str = "\
    Unidentified Alt AltGraph CapsLock Control Fn FnLock Hyper Meta NumLock ScrollLock Shift Super Symbol SymbolLock OS Win Enter \
    Tab ArrowDown ArrowLeft ArrowRight ArrowUp End Home PageDown PageUp Backspace Clear Copy CrSel Cut Delete EraseEof ExSel \
    Insert Paste Redo Undo Accept Again Attn Cancel ContextMenu Esc Escape Execute Find Finish Help Pause Play Props Select \
    ZoomIn ZoomOut BrightnessDown BrightnessUp Eject LogOff Power PowerOff PrintScreen Hibernate Standby WakeUp AllCandidates \
    Alphanumeric CodeInput Compose Convert Dead FinalMode GroupFirst GroupLast GroupNext GroupPrevious ModeChange NextCandidate \
    NonConvert PreviousCandidate Process SingleCandidate HangulMode HanjaMode JunjaMode Eisu Hankaku Hiragana HiraganaKatakana \
    KanaMode KanjiMode Katakana Romaji Zenkaku ZenkakuHankaku Soft1 Soft2 Soft3 Soft4 ChannelDown ChannelUp Close MailForward \
    MailReply MailSend MediaClose MediaFastForward MediaPause MediaPlay MediaPlayPause MediaRecord MediaRewind MediaStop \
    MediaTrackNext MediaTrackPrevious New Open Print Save SpellCheck Key11 Key12 AudioBalanceLeft AudioBalanceRight \
    AudioBassBoostDown AudioBassBoostToggle AudioBassBoostUp AudioFaderFront AudioFaderRear AudioSurroundModeNext AudioTrebleDown \
    AudioTrebleUp AudioVolumeDown AudioVolumeUp AudioVolumeMute MicrophoneToggle MicrophoneVolumeDown MicrophoneVolumeUp \
    MicrophoneVolumeMute SpeechCorrectionList SpeechInputToggle LaunchApplication1 LaunchApplication2 LaunchCalendar \
    LaunchContacts LaunchMail LaunchMediaPlayer LaunchMusicPlayer LaunchPhone LaunchScreenSaver LaunchSpreadsheet \
    LaunchWebBrowser LaunchWebCam LaunchWordProcessor BrowserBack BrowserFavorites BrowserForward BrowserHome BrowserRefresh \
    BrowserSearch BrowserStop AppSwitch Call Camera CameraFocus EndCall GoBack GoHome HeadsetHook LastNumberRedial Notification \
    MannerMode VoiceDial TV TV3DMode TVAntennaCable TVAudioDescription TVAudioDescriptionMixDown TVAudioDescriptionMixUp \
    TVContentsMenu TVDataService TVInput TVInputComponent1 TVInputComponent2 TVInputComposite1 TVInputComposite2 TVInputHDMI1 \
    TVInputHDMI2 TVInputHDMI3 TVInputHDMI4 TVInputVGA1 TVMediaContext TVNetwork TVNumberEntry TVPower TVRadioService TVSatellite \
    TVSatelliteBS TVSatelliteCS TVSatelliteToggle TVTerrestrialAnalog TVTerrestrialDigital TVTimer AVRInput AVRPower ColorF0Red \
    ColorF1Green ColorF2Yellow ColorF3Blue ColorF4Grey ColorF5Brown ClosedCaptionToggle Dimmer DisplaySwap DVR Exit \
    FavoriteClear0 FavoriteClear1 FavoriteClear2 FavoriteClear3 FavoriteRecall0 FavoriteRecall1 FavoriteRecall2 FavoriteRecall3 \
    FavoriteStore0 FavoriteStore1 FavoriteStore2 FavoriteStore3 Guide GuideNextDay GuidePreviousDay Info InstantReplay Link \
    ListProgram LiveContent Lock MediaApps MediaAudioTrack MediaLast MediaSkipBackward MediaSkipForward MediaStepBackward \
    MediaStepForward MediaTopMenu NavigateIn NavigateNext NavigateOut NavigatePrevious NextFavoriteChannel NextUserProfile \
    OnDemand Pairing PinPDown PinPMove PinPToggle PinPUp PlaySpeedDown PlaySpeedReset PlaySpeedUp RandomToggle RcLowBattery \
    RecordSpeedNext RfBypass ScanChannelsToggle ScreenModeNext Settings SplitScreenToggle STBInput STBPower Subtitle Teletext \
    VideoModeNext Wink ZoomToggle";

/// Is `text` the name of a key ("AltGraph", "Dead", "F13") instead of typed text?
pub fn is_key_name(text: &str) -> bool {
    let function_key = text.strip_prefix('F').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    function_key || KEY_NAMES.split_ascii_whitespace().any(|k| k == text)
}

/// Drop text events that are key names: eframe's web backend sends the name of every key it
/// doesn't know as text, so AltGr typed "AltGraph" into the story. `text_input_focused`: an egui
/// text field or the canvas's text caret has the keyboard, so the browser's hidden text input
/// sends the text, which holds only typed text (whole words from autocompletion too) and is left
/// alone.
pub fn drop_key_name_text(raw: &mut egui::RawInput, text_input_focused: bool) {
    if !text_input_focused {
        raw.events.retain(|e| !matches!(e, egui::Event::Text(t) if is_key_name(t)));
    }
}

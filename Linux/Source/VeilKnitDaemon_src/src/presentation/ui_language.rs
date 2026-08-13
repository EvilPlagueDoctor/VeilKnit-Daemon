use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    sync::atomic::{AtomicU8, Ordering},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum UiLanguage {
    English = 0,
    French = 1,
    Spanish = 2,
    Russian = 3,
    Chinese = 4,
}

static CURRENT_LANGUAGE: AtomicU8 = AtomicU8::new(UiLanguage::English as u8);

impl UiLanguage {
    pub const ALL: [UiLanguage; 5] = [
        UiLanguage::English,
        UiLanguage::French,
        UiLanguage::Spanish,
        UiLanguage::Russian,
        UiLanguage::Chinese,
    ];

    pub fn code(self) -> &'static str {
        match self {
            UiLanguage::English => "en",
            UiLanguage::French => "fr",
            UiLanguage::Spanish => "es",
            UiLanguage::Russian => "ru",
            UiLanguage::Chinese => "zh",
        }
    }

    pub fn native_name(self) -> &'static str {
        match self {
            UiLanguage::English => "English",
            UiLanguage::French => "Français",
            UiLanguage::Spanish => "Español",
            UiLanguage::Russian => "Русский",
            UiLanguage::Chinese => "中文",
        }
    }

    fn from_code(value: &str) -> UiLanguage {
        match value.trim().to_ascii_lowercase().as_str() {
            "fr" => UiLanguage::French,
            "es" => UiLanguage::Spanish,
            "ru" => UiLanguage::Russian,
            "zh" | "cn" => UiLanguage::Chinese,
            _ => UiLanguage::English,
        }
    }
}

pub fn current() -> UiLanguage {
    match CURRENT_LANGUAGE.load(Ordering::Relaxed) {
        1 => UiLanguage::French,
        2 => UiLanguage::Spanish,
        3 => UiLanguage::Russian,
        4 => UiLanguage::Chinese,
        _ => UiLanguage::English,
    }
}

pub fn set(language: UiLanguage) {
    CURRENT_LANGUAGE.store(language as u8, Ordering::Relaxed);
}

fn settings_path() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("veilknit")
            .join("language");
    }
    PathBuf::from("veilknit_language.txt")
}

pub fn load() -> UiLanguage {
    fs::read_to_string(settings_path())
        .map(|value| UiLanguage::from_code(&value))
        .unwrap_or(UiLanguage::English)
}

pub fn save(language: UiLanguage) {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, language.code());
}

/// Console language chooser shown before login. Pressing Enter keeps the
/// previously saved language (English on first run).
pub fn select_console_language() {
    let saved = load();
    println!("Select interface language / Choisir la langue / Seleccione el idioma / Выберите язык / 选择语言:");
    for (index, language) in UiLanguage::ALL.iter().enumerate() {
        let marker = if *language == saved { " *" } else { "" };
        println!("  {}. {}{}", index + 1, language.native_name(), marker);
    }
    print!("Language [1-5, Enter = {}]: ", saved.native_name());
    let _ = io::stdout().flush();
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
    let selected = match input.trim() {
        "1" => UiLanguage::English,
        "2" => UiLanguage::French,
        "3" => UiLanguage::Spanish,
        "4" => UiLanguage::Russian,
        "5" => UiLanguage::Chinese,
        _ => saved,
    };
    set(selected);
    save(selected);
    println!();
}

pub fn t(english: &'static str) -> &'static str {
    match current() {
        UiLanguage::English => english,
        UiLanguage::French => match english {
            "Login or Signup? (l/s): " => "Connexion ou création de compte ? (l/s) : ",
            "Username: " => "Nom d’utilisateur : ",
            "Password: " => "Mot de passe : ",
            "Please enter l or s." => "Saisissez l ou s.",
            "No account with that username." => "Aucun compte ne porte ce nom.",
            "That username is already taken." => "Ce nom d’utilisateur est déjà utilisé.",
            "Wrong password." => "Mot de passe incorrect.",
            "Usernames may only contain letters, numbers, '_' and '-'." => "Le nom d’utilisateur ne peut contenir que des lettres, chiffres, '_' et '-'.",
            "Command: " => "Commande : ",
            "Core commands:" => "Commandes principales :",
            "Mailbox status, inbox, retrieval, and maintenance" => "État de la boîte, réception, récupération et maintenance",
            "Review first-run app authorization" => "Examiner l’autorisation initiale des applications",
            "Manage authenticated local applications" => "Gérer les applications locales authentifiées",
            "Save the current session log" => "Enregistrer le journal de session",
            "Start, inspect, or stop a network walk" => "Démarrer, inspecter ou arrêter un parcours réseau",
            "Show internal nodes" => "Afficher les nœuds internes",
            "Show daemon and DHT status" => "Afficher l’état du démon et de la DHT",
            "Save and shut down" => "Enregistrer et arrêter",
            "No application authorization requests are pending." => "Aucune demande d’autorisation d’application n’est en attente.",
            "Pending application authorization requests:" => "Demandes d’autorisation d’application en attente :",
            "Approve with: app-approve <request-id>" => "Approuver avec : app-approve <identifiant-demande>",
            "Reject with: app-reject <request-id> [reason]" => "Refuser avec : app-reject <identifiant-demande> [motif]",
            "Application linking:" => "Association d’une application :",
            "Open the app once, note its request number, then use app-pending and app-approve <request-id>." => "Ouvrez l’application une fois, notez son numéro de demande, puis utilisez app-pending et app-approve <identifiant-demande>.",
            "Network" => "Réseau",
            "Attached" => "Attaché",
            "Uptime" => "Durée",
            "verified" => "vérifiée",
            "pending" => "en attente",
            "Handshake" => "Connexion",
            "Walk" => "Parcours",
            "Mail" => "Courrier",
            "Last" => "Dernier événement",
            "Recent activity" => "Activité récente",
            "Commands" => "Commandes",
            "Working..." => "En cours…",
            "yes" => "oui",
            "no" => "non",
            "help | mail | H handshake | T walk | U save-log | Q quit" => "help | mail | H connexion | T parcours | U journal | Q quitter",
            "N New  G Inspect  W Write  A Write-all  R/L Owned-read  E/X/Y External-read" => "N Nouveau  G Inspecter  W Écrire  A Tout-écrire  R/L Lecture locale  E/X/Y Lecture externe",
            "F2 full commands | PgUp/PgDn activity | End latest" => "F2 commandes complètes | PgUp/PgDn activité | End récent",
            "S Save  C Route  D Debug  H Handshake  K Handshake-status  T/P/O Walk" => "S Enregistrer  C Route  D Diagnostic  H Connexion  K État-connexion  T/P/O Parcours",
            "I Nodes  mail ... Mailbox  V App-reputation  Z Retract-app  U Save-log  Q Quit" => "I Nœuds  mail ... Boîte  V Réputation-app  Z Retirer-app  U Journal  Q Quitter",
            "Enter submit | arrows edit | PgUp/PgDn scroll | End latest | F2 compact" => "Entrée valider | flèches modifier | PgUp/PgDn défiler | End récent | F2 compact",
            "Language" => "Langue",
            _ => english,
        },
        UiLanguage::Spanish => match english {
            "Login or Signup? (l/s): " => "¿Iniciar sesión o crear cuenta? (l/s): ",
            "Username: " => "Nombre de usuario: ",
            "Password: " => "Contraseña: ",
            "Please enter l or s." => "Escriba l o s.",
            "No account with that username." => "No existe una cuenta con ese nombre.",
            "That username is already taken." => "Ese nombre de usuario ya está en uso.",
            "Wrong password." => "Contraseña incorrecta.",
            "Usernames may only contain letters, numbers, '_' and '-'." => "El nombre solo puede contener letras, números, '_' y '-'.",
            "Command: " => "Comando: ",
            "Core commands:" => "Comandos principales:",
            "Mailbox status, inbox, retrieval, and maintenance" => "Estado, bandeja, recuperación y mantenimiento del buzón",
            "Review first-run app authorization" => "Revisar la autorización inicial de aplicaciones",
            "Manage authenticated local applications" => "Administrar aplicaciones locales autenticadas",
            "Save the current session log" => "Guardar el registro de la sesión",
            "Start, inspect, or stop a network walk" => "Iniciar, revisar o detener un recorrido de red",
            "Show internal nodes" => "Mostrar nodos internos",
            "Show daemon and DHT status" => "Mostrar estado del demonio y DHT",
            "Save and shut down" => "Guardar y apagar",
            "No application authorization requests are pending." => "No hay solicitudes de autorización pendientes.",
            "Pending application authorization requests:" => "Solicitudes de autorización pendientes:",
            "Approve with: app-approve <request-id>" => "Aprobar con: app-approve <id-solicitud>",
            "Reject with: app-reject <request-id> [reason]" => "Rechazar con: app-reject <id-solicitud> [motivo]",
            "Application linking:" => "Vinculación de aplicaciones:",
            "Open the app once, note its request number, then use app-pending and app-approve <request-id>." => "Abra la aplicación una vez, anote su número de solicitud y use app-pending y app-approve <id-solicitud>.",
            "Network" => "Red",
            "Attached" => "Conectado",
            "Uptime" => "Tiempo activo",
            "verified" => "verificada",
            "pending" => "pendiente",
            "Handshake" => "Enlace",
            "Walk" => "Recorrido",
            "Mail" => "Correo",
            "Last" => "Último evento",
            "Recent activity" => "Actividad reciente",
            "Commands" => "Comandos",
            "Working..." => "Trabajando…",
            "yes" => "sí",
            "no" => "no",
            "help | mail | H handshake | T walk | U save-log | Q quit" => "help | mail | H enlace | T recorrido | U guardar-registro | Q salir",
            "N New  G Inspect  W Write  A Write-all  R/L Owned-read  E/X/Y External-read" => "N Nuevo  G Inspeccionar  W Escribir  A Escribir-todo  R/L Lectura propia  E/X/Y Lectura externa",
            "F2 full commands | PgUp/PgDn activity | End latest" => "F2 comandos completos | PgUp/PgDn actividad | End reciente",
            "S Save  C Route  D Debug  H Handshake  K Handshake-status  T/P/O Walk" => "S Guardar  C Ruta  D Diagnóstico  H Enlace  K Estado-enlace  T/P/O Recorrido",
            "I Nodes  mail ... Mailbox  V App-reputation  Z Retract-app  U Save-log  Q Quit" => "I Nodos  mail ... Buzón  V Reputación-app  Z Retirar-app  U Guardar-registro  Q Salir",
            "Enter submit | arrows edit | PgUp/PgDn scroll | End latest | F2 compact" => "Enter enviar | flechas editar | PgUp/PgDn desplazar | End reciente | F2 compacto",
            "Language" => "Idioma",
            _ => english,
        },
        UiLanguage::Russian => match english {
            "Login or Signup? (l/s): " => "Войти или создать учётную запись? (l/s): ",
            "Username: " => "Имя пользователя: ",
            "Password: " => "Пароль: ",
            "Please enter l or s." => "Введите l или s.",
            "No account with that username." => "Учётная запись с таким именем не найдена.",
            "That username is already taken." => "Это имя пользователя уже занято.",
            "Wrong password." => "Неверный пароль.",
            "Usernames may only contain letters, numbers, '_' and '-'." => "Имя может содержать только буквы, цифры, '_' и '-'.",
            "Command: " => "Команда: ",
            "Core commands:" => "Основные команды:",
            "Mailbox status, inbox, retrieval, and maintenance" => "Состояние почты, входящие, получение и обслуживание",
            "Review first-run app authorization" => "Проверка первого запроса авторизации приложения",
            "Manage authenticated local applications" => "Управление авторизованными локальными приложениями",
            "Save the current session log" => "Сохранить журнал сеанса",
            "Start, inspect, or stop a network walk" => "Запустить, проверить или остановить обход сети",
            "Show internal nodes" => "Показать внутренние узлы",
            "Show daemon and DHT status" => "Показать состояние демона и DHT",
            "Save and shut down" => "Сохранить и завершить работу",
            "No application authorization requests are pending." => "Нет ожидающих запросов авторизации приложений.",
            "Pending application authorization requests:" => "Ожидающие запросы авторизации приложений:",
            "Approve with: app-approve <request-id>" => "Одобрить: app-approve <номер-запроса>",
            "Reject with: app-reject <request-id> [reason]" => "Отклонить: app-reject <номер-запроса> [причина]",
            "Application linking:" => "Подключение приложения:",
            "Open the app once, note its request number, then use app-pending and app-approve <request-id>." => "Откройте приложение, запишите номер запроса, затем выполните app-pending и app-approve <номер-запроса>.",
            "Network" => "Сеть",
            "Attached" => "Подключение",
            "Uptime" => "Время работы",
            "verified" => "проверено",
            "pending" => "ожидание",
            "Handshake" => "Рукопожатие",
            "Walk" => "Обход",
            "Mail" => "Почта",
            "Last" => "Последнее событие",
            "Recent activity" => "Последние события",
            "Commands" => "Команды",
            "Working..." => "Работа…",
            "yes" => "да",
            "no" => "нет",
            "help | mail | H handshake | T walk | U save-log | Q quit" => "help | mail | H рукопожатие | T обход | U сохранить-журнал | Q выход",
            "N New  G Inspect  W Write  A Write-all  R/L Owned-read  E/X/Y External-read" => "N Создать  G Проверить  W Записать  A Записать-всё  R/L Свои записи  E/X/Y Внешние записи",
            "F2 full commands | PgUp/PgDn activity | End latest" => "F2 все команды | PgUp/PgDn события | End последние",
            "S Save  C Route  D Debug  H Handshake  K Handshake-status  T/P/O Walk" => "S Сохранить  C Маршрут  D Диагностика  H Рукопожатие  K Состояние  T/P/O Обход",
            "I Nodes  mail ... Mailbox  V App-reputation  Z Retract-app  U Save-log  Q Quit" => "I Узлы  mail ... Почта  V Репутация-app  Z Отозвать-app  U Журнал  Q Выход",
            "Enter submit | arrows edit | PgUp/PgDn scroll | End latest | F2 compact" => "Enter отправить | стрелки правка | PgUp/PgDn прокрутка | End последние | F2 компактно",
            "Language" => "Язык",
            _ => english,
        },
        UiLanguage::Chinese => match english {
            "Login or Signup? (l/s): " => "登录或创建账户？(l/s)：",
            "Username: " => "用户名：",
            "Password: " => "密码：",
            "Please enter l or s." => "请输入 l 或 s。",
            "No account with that username." => "没有该用户名的账户。",
            "That username is already taken." => "该用户名已被使用。",
            "Wrong password." => "密码错误。",
            "Usernames may only contain letters, numbers, '_' and '-'." => "用户名只能包含字母、数字、'_' 和 '-'。",
            "Command: " => "命令：",
            "Core commands:" => "主要命令：",
            "Mailbox status, inbox, retrieval, and maintenance" => "邮箱状态、收件箱、检索和维护",
            "Review first-run app authorization" => "查看应用首次运行授权",
            "Manage authenticated local applications" => "管理已认证的本地应用",
            "Save the current session log" => "保存当前会话日志",
            "Start, inspect, or stop a network walk" => "开始、检查或停止网络遍历",
            "Show internal nodes" => "显示内部节点",
            "Show daemon and DHT status" => "显示守护进程和 DHT 状态",
            "Save and shut down" => "保存并关闭",
            "No application authorization requests are pending." => "没有待处理的应用授权请求。",
            "Pending application authorization requests:" => "待处理的应用授权请求：",
            "Approve with: app-approve <request-id>" => "批准：app-approve <请求编号>",
            "Reject with: app-reject <request-id> [reason]" => "拒绝：app-reject <请求编号> [原因]",
            "Application linking:" => "应用连接：",
            "Open the app once, note its request number, then use app-pending and app-approve <request-id>." => "先打开应用并记下请求编号，然后使用 app-pending 和 app-approve <请求编号>。",
            "Network" => "网络",
            "Attached" => "已连接",
            "Uptime" => "运行时间",
            "verified" => "已验证",
            "pending" => "等待中",
            "Handshake" => "握手",
            "Walk" => "遍历",
            "Mail" => "邮件",
            "Last" => "最后事件",
            "Recent activity" => "最近活动",
            "Commands" => "命令",
            "Working..." => "处理中…",
            "yes" => "是",
            "no" => "否",
            "help | mail | H handshake | T walk | U save-log | Q quit" => "help | mail | H 握手 | T 遍历 | U 保存日志 | Q 退出",
            "N New  G Inspect  W Write  A Write-all  R/L Owned-read  E/X/Y External-read" => "N 新建  G 检查  W 写入  A 全部写入  R/L 自有读取  E/X/Y 外部读取",
            "F2 full commands | PgUp/PgDn activity | End latest" => "F2 完整命令 | PgUp/PgDn 活动 | End 最新",
            "S Save  C Route  D Debug  H Handshake  K Handshake-status  T/P/O Walk" => "S 保存  C 路由  D 诊断  H 握手  K 握手状态  T/P/O 遍历",
            "I Nodes  mail ... Mailbox  V App-reputation  Z Retract-app  U Save-log  Q Quit" => "I 节点  mail ... 邮箱  V 应用信誉  Z 撤回应用  U 保存日志  Q 退出",
            "Enter submit | arrows edit | PgUp/PgDn scroll | End latest | F2 compact" => "Enter 提交 | 方向键编辑 | PgUp/PgDn 滚动 | End 最新 | F2 紧凑",
            "Language" => "语言",
            _ => english,
        },
    }
}

#pragma once
#include <cstring>

enum class UiLanguage { English = 0, French = 1, Spanish = 2, Russian = 3, Chinese = 4 };

inline const char* language_code(UiLanguage language) {
    switch (language) {
        case UiLanguage::French: return "fr";
        case UiLanguage::Spanish: return "es";
        case UiLanguage::Russian: return "ru";
        case UiLanguage::Chinese: return "zh";
        default: return "en";
    }
}

inline UiLanguage language_from_code(const char* code) {
    if (!code) return UiLanguage::English;
    if (std::strcmp(code, "fr") == 0) return UiLanguage::French;
    if (std::strcmp(code, "es") == 0) return UiLanguage::Spanish;
    if (std::strcmp(code, "ru") == 0) return UiLanguage::Russian;
    if (std::strcmp(code, "zh") == 0) return UiLanguage::Chinese;
    return UiLanguage::English;
}

inline const char* language_native_name(UiLanguage language) {
    switch (language) {
        case UiLanguage::French: return "Français";
        case UiLanguage::Spanish: return "Español";
        case UiLanguage::Russian: return "Русский";
        case UiLanguage::Chinese: return "中文";
        default: return "English";
    }
}

struct TranslationRow { const char* en; const char* fr; const char* es; const char* ru; const char* zh; };

inline const char* ui_text(UiLanguage language, const char* english) {
    static const TranslationRow rows[] = {
        {"VeilKnit Daemon", "Démon VeilKnit", "Demonio VeilKnit", "Демон VeilKnit", "VeilKnit 守护进程"},
        {"Language", "Langue", "Idioma", "Язык", "语言"},
        {"Username", "Nom d’utilisateur", "Nombre de usuario", "Имя пользователя", "用户名"},
        {"Password", "Mot de passe", "Contraseña", "Пароль", "密码"},
        {"Log in", "Se connecter", "Iniciar sesión", "Войти", "登录"},
        {"Create account", "Créer un compte", "Crear cuenta", "Создать учётную запись", "创建账户"},
        {"Overview", "Vue d’ensemble", "Resumen", "Обзор", "概览"},
        {"Network", "Réseau", "Red", "Сеть", "网络"},
        {"Headers", "En-têtes", "Encabezados", "Заголовки", "标头"},
        {"Applications", "Applications", "Aplicaciones", "Приложения", "应用程序"},
        {"All logs", "Tous les journaux", "Todos los registros", "Все журналы", "所有日志"},
        {"Status", "État", "Estado", "Состояние", "状态"},
        {"Waiting for login", "En attente de connexion", "Esperando inicio de sesión", "Ожидание входа", "等待登录"},
        {"Starting backend…", "Démarrage du backend…", "Iniciando backend…", "Запуск backend…", "正在启动后端…"},
        {"Authenticated; starting network services…", "Authentifié ; démarrage des services réseau…", "Autenticado; iniciando servicios de red…", "Вход выполнен; запуск сетевых служб…", "已认证；正在启动网络服务…"},
        {"Running", "En cours", "En ejecución", "Работает", "运行中"},
        {"Authentication failed. Check the username and password.", "Échec de l’authentification. Vérifiez le nom et le mot de passe.", "Error de autenticación. Revise el nombre y la contraseña.", "Ошибка входа. Проверьте имя и пароль.", "认证失败。请检查用户名和密码。"},
        {"Backend stopped", "Backend arrêté", "Backend detenido", "Backend остановлен", "后端已停止"},
        {"Main DHT key", "Clé DHT principale", "Clave DHT principal", "Основной ключ DHT", "主 DHT 密钥"},
        {"Copy key", "Copier la clé", "Copiar clave", "Копировать ключ", "复制密钥"},
        {"Help", "Aide", "Ayuda", "Справка", "帮助"},
        {"Save log", "Enregistrer le journal", "Guardar registro", "Сохранить журнал", "保存日志"},
        {"Stop safely", "Arrêter proprement", "Detener con seguridad", "Безопасно остановить", "安全停止"},
        {"Normal walking mode", "Mode de parcours normal", "Modo de recorrido normal", "Обычный режим обхода", "普通遍历模式"},
        {"Mail walking mode", "Mode de parcours courrier", "Modo de recorrido de correo", "Почтовый режим обхода", "邮件遍历模式"},
        {"Minimum hops", "Sauts minimum", "Saltos mínimos", "Минимум переходов", "最少跳数"},
        {"Maximum hops", "Sauts maximum", "Saltos máximos", "Максимум переходов", "最大跳数"},
        {"Minimum interval (seconds)", "Intervalle minimum (secondes)", "Intervalo mínimo (segundos)", "Минимальный интервал (секунды)", "最小间隔（秒）"},
        {"Target interval (seconds)", "Intervalle cible (secondes)", "Intervalo objetivo (segundos)", "Целевой интервал (секунды)", "目标间隔（秒）"},
        {"Maximum interval (seconds)", "Intervalle maximum (secondes)", "Intervalo máximo (segundos)", "Максимальный интервал (секунды)", "最大间隔（秒）"},
        {"Use mail mode for automatic walks", "Utiliser le mode courrier pour les parcours automatiques", "Usar modo correo para recorridos automáticos", "Использовать почтовый режим для автоматических обходов", "自动遍历使用邮件模式"},
        {"Apply and save walk settings", "Appliquer et enregistrer les paramètres", "Aplicar y guardar ajustes", "Применить и сохранить настройки", "应用并保存遍历设置"},
        {"Start normal walk", "Démarrer le parcours normal", "Iniciar recorrido normal", "Начать обычный обход", "开始普通遍历"},
        {"Start mail walk", "Démarrer le parcours courrier", "Iniciar recorrido de correo", "Начать почтовый обход", "开始邮件遍历"},
        {"Walk status", "État du parcours", "Estado del recorrido", "Состояние обхода", "遍历状态"},
        {"Stop walk", "Arrêter le parcours", "Detener recorrido", "Остановить обход", "停止遍历"},
        {"Route status", "État des routes", "Estado de rutas", "Состояние маршрутов", "路由状态"},
        {"Node list", "Liste des nœuds", "Lista de nodos", "Список узлов", "节点列表"},
        {"Daemon status", "État du démon", "Estado del demonio", "Состояние демона", "守护进程状态"},
        {"Published main/presence header (subkey 0)", "En-tête principal/de présence publié (sous-clé 0)", "Encabezado principal/presencia publicado (subclave 0)", "Опубликованный заголовок присутствия (подключ 0)", "已发布的主/在线状态标头（子键 0）"},
        {"Published mailbox advertisement (subkey 2)", "Annonce de boîte aux lettres publiée (sous-clé 2)", "Anuncio de buzón publicado (subclave 2)", "Опубликованное объявление почтового ящика (подключ 2)", "已发布的邮箱公告（子键 2）"},
        {"Copy main header", "Copier l’en-tête principal", "Copiar encabezado principal", "Копировать основной заголовок", "复制主标头"},
        {"Copy mailbox header", "Copier l’en-tête de boîte", "Copiar encabezado del buzón", "Копировать заголовок почты", "复制邮箱标头"},
        {"Refresh both headers", "Actualiser les deux en-têtes", "Actualizar ambos encabezados", "Обновить оба заголовка", "刷新两个标头"},
        {"Local applications", "Applications locales", "Aplicaciones locales", "Локальные приложения", "本地应用程序"},
        {"Application id", "Identifiant d’application", "Id. de aplicación", "Идентификатор приложения", "应用程序 ID"},
        {"Display name", "Nom affiché", "Nombre mostrado", "Отображаемое имя", "显示名称"},
        {"Register", "Enregistrer", "Registrar", "Зарегистрировать", "注册"},
        {"List", "Lister", "Listar", "Список", "列表"},
        {"Rotate selected app key", "Faire tourner la clé de l’application", "Rotar clave de la aplicación", "Сменить ключ приложения", "轮换应用密钥"},
        {"Registration requests", "Demandes d’enregistrement", "Solicitudes de registro", "Запросы регистрации", "注册请求"},
        {"Show pending requests", "Afficher les demandes en attente", "Mostrar solicitudes pendientes", "Показать ожидающие запросы", "显示待处理请求"},
        {"Request id", "Identifiant de demande", "Id. de solicitud", "Номер запроса", "请求 ID"},
        {"Rejection reason", "Motif du refus", "Motivo del rechazo", "Причина отказа", "拒绝原因"},
        {"Approve", "Approuver", "Aprobar", "Одобрить", "批准"},
        {"Reject", "Refuser", "Rechazar", "Отклонить", "拒绝"},
        {"Applications must be approved before they can use your identity.", "Les applications doivent être approuvées avant d’utiliser votre identité.", "Las aplicaciones deben aprobarse antes de usar su identidad.", "Приложения нужно одобрить, прежде чем они смогут использовать вашу личность.", "应用程序必须先获批准，才能使用您的身份。"},
        {"Open the new app once, then press Show pending requests. Enter its request number and press Approve.", "Ouvrez la nouvelle application une fois, puis affichez les demandes. Saisissez son numéro et appuyez sur Approuver.", "Abra la aplicación una vez, muestre las solicitudes, escriba su número y pulse Aprobar.", "Откройте новое приложение, покажите ожидающие запросы, введите номер и нажмите Одобрить.", "先打开新应用一次，然后显示待处理请求。输入其请求编号并点击批准。"},
        {"Getting started", "Pour commencer", "Primeros pasos", "Начало работы", "入门"},
        {"Enter a username and password, then log in or create an account. To link an app, open it once, go to Applications, show pending requests, enter the request number, and approve it. Network contains separate normal and mail walk controls. Headers displays the published presence and mailbox records.", "Saisissez un nom et un mot de passe, puis connectez-vous ou créez un compte. Pour associer une application, ouvrez-la une fois, allez dans Applications, affichez les demandes, saisissez le numéro et approuvez-la. Réseau contient les contrôles des parcours normal et courrier. En-têtes affiche les enregistrements publiés de présence et de boîte.", "Escriba un nombre y contraseña e inicie sesión o cree una cuenta. Para vincular una aplicación, ábrala una vez, vaya a Aplicaciones, muestre solicitudes, escriba el número y apruébela. Red contiene controles separados de recorrido normal y de correo. Encabezados muestra los registros publicados.", "Введите имя и пароль, затем войдите или создайте учётную запись. Чтобы подключить приложение, откройте его, перейдите в Приложения, покажите запросы, введите номер и одобрите. В разделе Сеть есть отдельные настройки обычного и почтового обхода. Заголовки показывают опубликованные записи.", "输入用户名和密码，然后登录或创建账户。要连接应用，请先打开它一次，进入应用程序，显示待处理请求，输入请求编号并批准。网络页面包含普通和邮件遍历的独立控制。标头页面显示已发布的在线状态和邮箱记录。"},
        {"Cancel", "Annuler", "Cancelar", "Отмена", "取消"},
        {"Close", "Fermer", "Cerrar", "Закрыть", "关闭"},
        {"Not available yet", "Pas encore disponible", "Aún no disponible", "Пока недоступно", "尚不可用"},
        {"Copied", "Copié", "Copiado", "Скопировано", "已复制"},
        {"Settings sent", "Paramètres envoyés", "Ajustes enviados", "Настройки отправлены", "设置已发送"},
        {"The daemon is not ready yet.", "Le démon n’est pas encore prêt.", "El demonio aún no está listo.", "Демон ещё не готов.", "守护进程尚未就绪。"},
    };
    if (language == UiLanguage::English || !english) return english;
    for (const auto& row : rows) {
        if (std::strcmp(row.en, english) == 0) {
            switch (language) {
                case UiLanguage::French: return row.fr;
                case UiLanguage::Spanish: return row.es;
                case UiLanguage::Russian: return row.ru;
                case UiLanguage::Chinese: return row.zh;
                default: return row.en;
            }
        }
    }
    return english;
}

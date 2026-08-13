#include <gtk/gtk.h>
#include <unistd.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <signal.h>
#include <limits.h>
#include <fcntl.h>

#include <algorithm>
#include <cctype>
#include <initializer_list>
#include <memory>
#include <atomic>
#include <cerrno>
#include <cstring>
#include <fstream>
#include <mutex>
#include <sstream>
#include <string>
#include <thread>
#include <unordered_map>
#include <vector>

#include "ui_localization.h"

namespace {

UiLanguage g_language = UiLanguage::English;
GtkWidget* g_window = nullptr;
GtkWidget* g_login_frame = nullptr;
GtkWidget* g_language_label = nullptr;
GtkWidget* g_language_combo = nullptr;
GtkWidget* g_username_label = nullptr;
GtkWidget* g_username_entry = nullptr;
GtkWidget* g_password_label = nullptr;
GtkWidget* g_password_entry = nullptr;
GtkWidget* g_login_button = nullptr;
GtkWidget* g_signup_button = nullptr;
GtkWidget* g_restore_backup_button = nullptr;
GtkWidget* g_status_caption = nullptr;
GtkWidget* g_status_label = nullptr;
GtkWidget* g_notebook = nullptr;

GtkWidget* g_tab_overview = nullptr;
GtkWidget* g_tab_network = nullptr;
GtkWidget* g_tab_headers = nullptr;
GtkWidget* g_tab_apps = nullptr;
GtkWidget* g_tab_logs = nullptr;

GtkWidget* g_main_key_label = nullptr;
GtkWidget* g_main_key_entry = nullptr;
GtkWidget* g_copy_key_button = nullptr;
GtkWidget* g_help_button = nullptr;
GtkWidget* g_discord_button = nullptr;
GtkWidget* g_save_log_button = nullptr;
GtkWidget* g_stop_button = nullptr;

GtkWidget* g_normal_frame = nullptr;
GtkWidget* g_mail_frame = nullptr;
GtkWidget* g_normal_min_hops_label = nullptr;
GtkWidget* g_normal_max_hops_label = nullptr;
GtkWidget* g_normal_min_sec_label = nullptr;
GtkWidget* g_normal_target_sec_label = nullptr;
GtkWidget* g_normal_max_sec_label = nullptr;
GtkWidget* g_mail_min_hops_label = nullptr;
GtkWidget* g_mail_max_hops_label = nullptr;
GtkWidget* g_mail_min_sec_label = nullptr;
GtkWidget* g_mail_target_sec_label = nullptr;
GtkWidget* g_mail_max_sec_label = nullptr;
GtkWidget* g_normal_min_hops = nullptr;
GtkWidget* g_normal_max_hops = nullptr;
GtkWidget* g_normal_min_sec = nullptr;
GtkWidget* g_normal_target_sec = nullptr;
GtkWidget* g_normal_max_sec = nullptr;
GtkWidget* g_mail_min_hops = nullptr;
GtkWidget* g_mail_max_hops = nullptr;
GtkWidget* g_mail_min_sec = nullptr;
GtkWidget* g_mail_target_sec = nullptr;
GtkWidget* g_mail_max_sec = nullptr;
GtkWidget* g_mail_auto_check = nullptr;
GtkWidget* g_apply_walk_button = nullptr;
GtkWidget* g_start_normal_button = nullptr;
GtkWidget* g_start_mail_button = nullptr;
GtkWidget* g_walk_status_button = nullptr;
GtkWidget* g_stop_walk_button = nullptr;
GtkWidget* g_route_status_button = nullptr;
GtkWidget* g_node_list_button = nullptr;
GtkWidget* g_daemon_status_button = nullptr;

GtkWidget* g_summary_topology_frame = nullptr;
GtkWidget* g_summary_topology_label = nullptr;
GtkWidget* g_summary_presence_frame = nullptr;
GtkWidget* g_summary_presence_label = nullptr;
GtkWidget* g_summary_headers_frame = nullptr;
GtkWidget* g_summary_headers_label = nullptr;
GtkWidget* g_summary_activity_frame = nullptr;
GtkWidget* g_summary_activity_label = nullptr;

GtkWidget* g_main_header_label = nullptr;
GtkWidget* g_main_header_view = nullptr;
GtkWidget* g_copy_main_header_button = nullptr;
GtkWidget* g_mail_header_label = nullptr;
GtkWidget* g_mail_header_view = nullptr;
GtkWidget* g_copy_mail_header_button = nullptr;
GtkWidget* g_refresh_headers_button = nullptr;

GtkWidget* g_apps_frame = nullptr;
GtkWidget* g_app_id_label = nullptr;
GtkWidget* g_app_id_entry = nullptr;
GtkWidget* g_app_name_label = nullptr;
GtkWidget* g_app_name_entry = nullptr;
GtkWidget* g_app_register_button = nullptr;
GtkWidget* g_app_list_button = nullptr;
GtkWidget* g_app_rotate_button = nullptr;
GtkWidget* g_requests_frame = nullptr;
GtkWidget* g_app_help_label = nullptr;
GtkWidget* g_app_pending_button = nullptr;
GtkWidget* g_request_id_label = nullptr;
GtkWidget* g_request_id_entry = nullptr;
GtkWidget* g_reject_reason_label = nullptr;
GtkWidget* g_reject_reason_entry = nullptr;
GtkWidget* g_approve_button = nullptr;
GtkWidget* g_reject_button = nullptr;
GtkListStore* g_request_store = nullptr;
GtkWidget* g_request_view = nullptr;
GtkWidget* g_found_apps_frame = nullptr;
GtkListStore* g_found_app_store = nullptr;
GtkWidget* g_found_app_view = nullptr;

GtkWidget* g_visible_names_frame = nullptr;
GtkWidget* g_visible_name_label = nullptr;
GtkWidget* g_visible_name_entry = nullptr;
GtkWidget* g_set_default_visible_name_button = nullptr;
GtkWidget* g_set_app_alias_button = nullptr;
GtkWidget* g_clear_app_alias_button = nullptr;
GtkWidget* g_list_visible_names_button = nullptr;

GtkWidget* g_profiles_frame = nullptr;
GtkWidget* g_apps_advanced_expander = nullptr;
GtkWidget* g_profile_name_label = nullptr;
GtkWidget* g_profile_name_entry = nullptr;
GtkWidget* g_profile_id_label = nullptr;
GtkWidget* g_profile_id_entry = nullptr;
GtkWidget* g_profile_create_button = nullptr;
GtkWidget* g_profile_list_button = nullptr;
GtkWidget* g_profile_use_button = nullptr;
GtkWidget* g_profile_retire_button = nullptr;
GtkWidget* g_profile_restart_note = nullptr;

GtkWidget* g_log_view = nullptr;

std::mutex g_process_mutex;
std::mutex g_write_mutex;
pid_t g_child_pid = -1;
int g_child_stdin = -1;
int g_child_stdout = -1;
std::thread g_reader_thread;
std::atomic<bool> g_process_running{false};
std::atomic<bool> g_ready{false};
std::atomic<bool> g_authenticated{false};

const char* T(const char* english) { return ui_text(g_language, english); }
bool require_ready();

std::string trim(std::string value) {
    while (!value.empty() && (value.back() == '\r' || value.back() == '\n' || value.back() == ' ' || value.back() == '\t')) value.pop_back();
    size_t start = 0;
    while (start < value.size() && (value[start] == ' ' || value[start] == '\t')) ++start;
    return value.substr(start);
}

std::string lower_ascii(std::string value) {
    std::transform(value.begin(), value.end(), value.begin(), [](unsigned char c) {
        return static_cast<char>(std::tolower(c));
    });
    return value;
}

std::string executable_directory() {
    char path[PATH_MAX]{};
    const ssize_t length = readlink("/proc/self/exe", path, sizeof(path) - 1);
    if (length <= 0) return ".";
    path[length] = '\0';
    std::string full(path);
    const auto slash = full.find_last_of('/');
    return slash == std::string::npos ? "." : full.substr(0, slash);
}

bool file_executable(const std::string& path) { return access(path.c_str(), X_OK) == 0; }

std::string backend_path() {
    const std::string directory = executable_directory();
    const std::vector<std::string> candidates = {
        directory + "/veilknit-daemon",
        directory + "/veilknit-daemon-console",
        directory + "/veilid_test_node",
        directory + "/../VeilKnitDaemon_src/target/release/veilid_test_node",
        directory + "/../../VeilKnitDaemon_src/target/release/veilid_test_node",
    };
    for (const auto& candidate : candidates) if (file_executable(candidate)) return candidate;
    return {};
}

std::string language_settings_path() {
    const char* config = g_get_user_config_dir();
    std::string directory = std::string(config ? config : ".") + "/veilknit";
    g_mkdir_with_parents(directory.c_str(), 0700);
    return directory + "/gui_language";
}

void load_language() {
    std::ifstream input(language_settings_path());
    std::string code;
    if (input >> code) g_language = language_from_code(code.c_str());
}

void save_language() {
    std::ofstream output(language_settings_path(), std::ios::trunc);
    output << language_code(g_language);
}

void set_status(const char* text) { gtk_label_set_text(GTK_LABEL(g_status_label), text); }

void enable_login(bool enabled) {
    gtk_widget_set_sensitive(g_username_entry, enabled);
    gtk_widget_set_sensitive(g_password_entry, enabled);
    gtk_widget_set_sensitive(g_login_button, enabled);
    gtk_widget_set_sensitive(g_signup_button, enabled);
    gtk_widget_set_sensitive(g_restore_backup_button, enabled);
}

void copy_text(const std::string& text) {
    GtkClipboard* clipboard = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    gtk_clipboard_set_text(clipboard, text.c_str(), static_cast<gint>(text.size()));
}

std::string entry_text(GtkWidget* entry) {
    const char* value = gtk_entry_get_text(GTK_ENTRY(entry));
    return value ? value : "";
}

std::string text_view_text(GtkWidget* view) {
    GtkTextBuffer* buffer = gtk_text_view_get_buffer(GTK_TEXT_VIEW(view));
    GtkTextIter start{}, end{};
    gtk_text_buffer_get_bounds(buffer, &start, &end);
    gchar* text = gtk_text_buffer_get_text(buffer, &start, &end, FALSE);
    std::string result = text ? text : "";
    g_free(text);
    return result;
}

void set_text_view(GtkWidget* view, const std::string& text) {
    gtk_text_buffer_set_text(gtk_text_view_get_buffer(GTK_TEXT_VIEW(view)), text.c_str(), -1);
}

void append_log(const std::string& line) {
    GtkTextBuffer* buffer = gtk_text_view_get_buffer(GTK_TEXT_VIEW(g_log_view));
    GtkTextIter end{};
    gtk_text_buffer_get_end_iter(buffer, &end);
    std::string output = line + "\n";
    gtk_text_buffer_insert(buffer, &end, output.c_str(), -1);
    GtkTextMark* mark = gtk_text_buffer_get_insert(buffer);
    gtk_text_view_scroll_mark_onscreen(GTK_TEXT_VIEW(g_log_view), mark);
}

bool send_line(const std::string& line) {
    std::lock_guard<std::mutex> lock(g_write_mutex);
    if (g_child_stdin < 0) return false;
    const std::string bytes = line + "\n";
    size_t offset = 0;
    while (offset < bytes.size()) {
        const ssize_t written = write(g_child_stdin, bytes.data() + offset, bytes.size() - offset);
        if (written < 0) {
            if (errno == EINTR) continue;
            return false;
        }
        offset += static_cast<size_t>(written);
    }
    return true;
}

void send_lines(std::initializer_list<std::string> lines) {
    for (const auto& line : lines) send_line(line);
}

std::string decode_gui_value(const std::string& value) {
    std::string result;
    bool escape = false;
    for (char c : value) {
        if (!escape) {
            if (c == '\\') escape = true;
            else result.push_back(c);
            continue;
        }
        switch (c) {
            case 'n': result.push_back('\n'); break;
            case 'r': result.push_back('\r'); break;
            case 't': result.push_back('\t'); break;
            case '\\': result.push_back('\\'); break;
            default: result.push_back(c); break;
        }
        escape = false;
    }
    if (escape) result.push_back('\\');
    return result;
}

void apply_walk_marker(const std::string& marker) {
    std::vector<long> values;
    std::stringstream stream(marker);
    std::string part;
    while (std::getline(stream, part, ',')) {
        try { values.push_back(std::stol(trim(part))); } catch (...) { return; }
    }
    if (values.size() != 11) return;
    GtkWidget* widgets[] = {g_normal_min_hops, g_normal_max_hops, g_normal_min_sec,
        g_normal_target_sec, g_normal_max_sec, g_mail_min_hops, g_mail_max_hops,
        g_mail_min_sec, g_mail_target_sec, g_mail_max_sec};
    for (size_t i = 0; i < 10; ++i) gtk_spin_button_set_value(GTK_SPIN_BUTTON(widgets[i]), values[i]);
    gtk_toggle_button_set_active(GTK_TOGGLE_BUTTON(g_mail_auto_check), values[10] != 0);
}

void show_message(GtkMessageType type, const char* title, const char* text) {
    GtkWidget* dialog = gtk_message_dialog_new(GTK_WINDOW(g_window), GTK_DIALOG_MODAL,
        type, GTK_BUTTONS_OK, "%s", text);
    gtk_window_set_title(GTK_WINDOW(dialog), title);
    gtk_dialog_run(GTK_DIALOG(dialog));
    gtk_widget_destroy(dialog);
}

void apply_language();

struct PostedLine { std::string line; };

unsigned long long summary_number(
    const std::unordered_map<std::string, std::string>& fields,
    const std::string& name) {
    auto found = fields.find(name);
    if (found == fields.end()) return 0;
    try { return std::stoull(found->second); } catch (...) { return 0; }
}


enum RequestColumns {
    REQ_CHECKED = 0,
    REQ_APP,
    REQ_NAME,
    REQ_ID,
    REQ_COLUMN_COUNT,
};

enum FoundAppColumns {
    FOUND_APP = 0,
    FOUND_OBSERVED,
    FOUND_CACHED,
    FOUND_RECENT,
    FOUND_ARCHIVE,
    FOUND_COLUMN_COUNT,
};

std::unordered_map<std::string, std::string> gui_fields(const std::string& marker) {
    std::unordered_map<std::string, std::string> fields;
    std::stringstream stream(marker);
    std::string part;
    while (std::getline(stream, part, ';')) {
        const auto equals = part.find('=');
        if (equals == std::string::npos || equals == 0) continue;
        fields[part.substr(0, equals)] = part.substr(equals + 1);
    }
    return fields;
}

std::string decode_hex_utf8(const std::string& value) {
    if ((value.size() % 2) != 0) return {};
    std::string result;
    result.reserve(value.size() / 2);
    auto nibble = [](char c) -> int {
        if (c >= '0' && c <= '9') return c - '0';
        if (c >= 'a' && c <= 'f') return 10 + c - 'a';
        if (c >= 'A' && c <= 'F') return 10 + c - 'A';
        return -1;
    };
    for (size_t i = 0; i < value.size(); i += 2) {
        const int high = nibble(value[i]);
        const int low = nibble(value[i + 1]);
        if (high < 0 || low < 0) return {};
        result.push_back(static_cast<char>((high << 4) | low));
    }
    return result;
}

unsigned long long field_number(
    const std::unordered_map<std::string, std::string>& fields,
    const std::string& key) {
    const auto found = fields.find(key);
    if (found == fields.end()) return 0;
    try { return std::stoull(found->second); } catch (...) { return 0; }
}

void append_pending_request_marker(const std::string& marker) {
    if (!g_request_store) return;
    const auto fields = gui_fields(marker);
    const auto app = fields.find("app_hex");
    const auto name = fields.find("name_hex");
    if (app == fields.end() || name == fields.end()) return;
    const std::string app_id = decode_hex_utf8(app->second);
    const std::string display_name = decode_hex_utf8(name->second);
    const guint64 request_id = static_cast<guint64>(field_number(fields, "request_id"));
    GtkTreeIter iter{};
    gtk_list_store_append(g_request_store, &iter);
    gtk_list_store_set(g_request_store, &iter,
        REQ_CHECKED, FALSE,
        REQ_APP, app_id.c_str(),
        REQ_NAME, display_name.c_str(),
        REQ_ID, request_id,
        -1);
}

void append_found_app_marker(const std::string& marker) {
    if (!g_found_app_store) return;
    const auto fields = gui_fields(marker);
    const auto app = fields.find("app_hex");
    if (app == fields.end()) return;
    const std::string app_id = decode_hex_utf8(app->second);
    GtkTreeIter iter{};
    gtk_list_store_append(g_found_app_store, &iter);
    gtk_list_store_set(g_found_app_store, &iter,
        FOUND_APP, app_id.c_str(),
        FOUND_OBSERVED, static_cast<guint>(field_number(fields, "observed")),
        FOUND_CACHED, static_cast<guint>(field_number(fields, "cached")),
        FOUND_RECENT, static_cast<guint>(field_number(fields, "recent")),
        FOUND_ARCHIVE, static_cast<guint>(field_number(fields, "archive")),
        -1);
}

void request_toggle(GtkCellRendererToggle*, gchar* path_text, gpointer) {
    if (!g_request_store) return;
    GtkTreePath* path = gtk_tree_path_new_from_string(path_text);
    GtkTreeIter iter{};
    if (path && gtk_tree_model_get_iter(GTK_TREE_MODEL(g_request_store), &iter, path)) {
        gboolean checked = FALSE;
        gtk_tree_model_get(GTK_TREE_MODEL(g_request_store), &iter, REQ_CHECKED, &checked, -1);
        gtk_list_store_set(g_request_store, &iter, REQ_CHECKED, !checked, -1);
    }
    if (path) gtk_tree_path_free(path);
}

void act_on_checked_requests(bool approve) {
    if (!require_ready() || !g_request_store) return;
    const std::string reason = g_reject_reason_entry ? entry_text(g_reject_reason_entry) : std::string{};
    GtkTreeIter iter{};
    gboolean valid = gtk_tree_model_get_iter_first(GTK_TREE_MODEL(g_request_store), &iter);
    bool sent = false;
    while (valid) {
        gboolean checked = FALSE;
        guint64 request_id = 0;
        gtk_tree_model_get(GTK_TREE_MODEL(g_request_store), &iter,
            REQ_CHECKED, &checked, REQ_ID, &request_id, -1);
        if (checked && request_id != 0) {
            if (approve) {
                send_line("app-approve " + std::to_string(request_id));
            } else {
                const std::string effective = reason.empty() ? "rejected by the local user" : reason;
                send_line("app-reject " + std::to_string(request_id) + " " + effective);
            }
            sent = true;
        }
        valid = gtk_tree_model_iter_next(GTK_TREE_MODEL(g_request_store), &iter);
    }
    if (sent) send_line("app-pending");
}

void apply_gui_summary(const std::string& marker) {
    std::unordered_map<std::string, std::string> fields;
    std::stringstream stream(marker);
    std::string part;
    while (std::getline(stream, part, ';')) {
        const auto equals = part.find('=');
        if (equals == std::string::npos || equals == 0) continue;
        fields[part.substr(0, equals)] = part.substr(equals + 1);
    }
    const auto n = [&](const std::string& key) { return summary_number(fields, key); };

    std::ostringstream topology;
    topology << T("Verified") << ": " << n("verified") << "\n"
             << T("Candidates") << ": " << n("candidates") << "\n"
             << T("Authenticated") << ": " << n("authenticated");
    gtk_label_set_text(GTK_LABEL(g_summary_topology_label), topology.str().c_str());

    std::ostringstream presence;
    presence << T("Online") << ": " << n("online") << "\n"
             << T("Offline") << ": " << n("offline") << "\n"
             << T("Stale claim") << ": " << n("stale") << "\n"
             << T("Needs refresh") << ": " << n("refresh") << "\n"
             << T("Unknown") << ": " << n("unknown");
    gtk_label_set_text(GTK_LABEL(g_summary_presence_label), presence.str().c_str());

    std::ostringstream headers;
    headers << T("Presence OK") << ": " << n("presence_ok") << "\n"
            << T("Read failed") << ": " << n("presence_failed") << "\n"
            << T("Not checked") << ": " << n("presence_unread") << "\n"
            << T("Active app info") << ": " << n("app_headers") << "\n"
            << T("Mailbox capable") << ": " << n("mailbox_capable");
    gtk_label_set_text(GTK_LABEL(g_summary_headers_label), headers.str().c_str());

    const std::string walk = fields.count("walk_state") ? fields["walk_state"] : "idle";
    std::ostringstream activity;
    activity << T("Walk") << ": " << walk;
    if (n("walk_total") > 0) activity << " " << n("walk_done") << "/" << n("walk_total");
    activity << "\n" << T("New / updated") << ": " << n("walk_new") << " / " << n("walk_updated")
             << "\n" << T("Reach / fail") << ": " << n("walk_reachable") << " / " << n("walk_unreachable")
             << "\n" << T("App searches") << ": " << n("app_searches")
             << "\n" << T("Root lookups") << ": " << n("root_lookups");
    gtk_label_set_text(GTK_LABEL(g_summary_activity_label), activity.str().c_str());
}

gboolean process_line_idle(gpointer data) {
    std::unique_ptr<PostedLine> posted(static_cast<PostedLine*>(data));
    const std::string& line = posted->line;
    const auto summary_marker = line.find("GUI_SUMMARY=");
    if (summary_marker != std::string::npos) {
        apply_gui_summary(line.substr(summary_marker + 12));
        return G_SOURCE_REMOVE;
    }
    if (line.find("GUI_APP_REQUESTS_BEGIN") != std::string::npos) {
        if (g_request_store) gtk_list_store_clear(g_request_store);
        return G_SOURCE_REMOVE;
    }
    const auto request_marker = line.find("GUI_APP_REQUEST=");
    if (request_marker != std::string::npos) {
        append_pending_request_marker(line.substr(request_marker + 16));
        return G_SOURCE_REMOVE;
    }
    if (line.find("GUI_APP_REQUESTS_END") != std::string::npos) return G_SOURCE_REMOVE;
    if (line.find("GUI_APPS_BEGIN") != std::string::npos) {
        if (g_found_app_store) gtk_list_store_clear(g_found_app_store);
        return G_SOURCE_REMOVE;
    }
    const auto found_app_marker = line.find("GUI_APP=");
    if (found_app_marker != std::string::npos) {
        append_found_app_marker(line.substr(found_app_marker + 8));
        return G_SOURCE_REMOVE;
    }
    if (line.find("GUI_APPS_END") != std::string::npos) return G_SOURCE_REMOVE;
    append_log(line);
    const std::string lower = lower_ascii(line);

    auto marker = line.find("MAIN_DHT_KEY=");
    if (marker != std::string::npos) {
        gtk_entry_set_text(GTK_ENTRY(g_main_key_entry), line.substr(marker + 13).c_str());
    }
    marker = line.find("WALK_SETTINGS=");
    if (marker != std::string::npos) apply_walk_marker(line.substr(marker + 14));
    marker = line.find("MAIN_HEADER=");
    if (marker != std::string::npos) set_text_view(g_main_header_view, decode_gui_value(line.substr(marker + 12)));
    marker = line.find("MAILBOX_HEADER=");
    if (marker != std::string::npos) set_text_view(g_mail_header_view, decode_gui_value(line.substr(marker + 15)));

    if (lower.find("welcome,") != std::string::npos) {
        g_authenticated = true;
        set_status(T("Authenticated; starting network services…"));
        gtk_entry_set_text(GTK_ENTRY(g_password_entry), "");
    }
    if (line.find("[gui] READY") != std::string::npos) {
        g_ready = true;
        enable_login(false);
        set_status(T("Running"));
        send_line("walk-settings");
        send_line("headers");
        send_line("summary");
        send_line("app-pending");
    }
    if (lower.find("no account with that username") != std::string::npos ||
        lower.find("wrong password") != std::string::npos ||
        lower.find("username is already taken") != std::string::npos ||
        lower.find("usernames may only contain") != std::string::npos) {
        g_authenticated = false;
        enable_login(true);
        set_status(T("Authentication failed. Check the username and password."));
    }
    if (lower.find("restored account") != std::string::npos ||
        lower.find("could not restore backup") != std::string::npos) {
        g_authenticated = false;
        enable_login(true);
        gtk_entry_set_text(GTK_ENTRY(g_password_entry), "");
        set_status(lower.find("restored account") != std::string::npos
            ? T("Backup restored. Log in with the original account password.")
            : T("Backup restore failed. Check the file and backup passphrase."));
    }
    return G_SOURCE_REMOVE;
}

gboolean backend_exit_idle(gpointer data) {
    const int status = GPOINTER_TO_INT(data);
    (void)status;
    g_process_running = false;
    g_ready = false;
    g_authenticated = false;
    g_child_pid = -1;
    if (g_child_stdin >= 0) { close(g_child_stdin); g_child_stdin = -1; }
    g_child_stdout = -1; // reader_loop owns and closes this descriptor
    enable_login(true);
    set_status(T("Backend stopped"));
    return G_SOURCE_REMOVE;
}

void reader_loop(int fd, pid_t pid) {
    FILE* stream = fdopen(fd, "r");
    if (stream) {
        char* line = nullptr;
        size_t capacity = 0;
        while (getline(&line, &capacity, stream) != -1) {
            std::string value = trim(line ? line : "");
            g_idle_add(process_line_idle, new PostedLine{value});
        }
        free(line);
        fclose(stream);
    }
    int status = 0;
    waitpid(pid, &status, 0);
    g_idle_add(backend_exit_idle, GINT_TO_POINTER(status));
}

bool start_backend() {
    std::lock_guard<std::mutex> lock(g_process_mutex);
    if (g_process_running) return true;
    if (g_reader_thread.joinable()) g_reader_thread.join();
    const std::string backend = backend_path();
    if (backend.empty()) {
        show_message(GTK_MESSAGE_ERROR, T("VeilKnit Daemon"),
            "The Rust backend was not found. Run build_gui.sh and keep veilknit-daemon beside this GUI.");
        return false;
    }

    int input_pipe[2]{};
    int output_pipe[2]{};
    if (pipe(input_pipe) != 0 || pipe(output_pipe) != 0) {
        show_message(GTK_MESSAGE_ERROR, T("VeilKnit Daemon"), std::strerror(errno));
        return false;
    }

    const pid_t child = fork();
    if (child < 0) {
        show_message(GTK_MESSAGE_ERROR, T("VeilKnit Daemon"), std::strerror(errno));
        return false;
    }
    if (child == 0) {
        dup2(input_pipe[0], STDIN_FILENO);
        dup2(output_pipe[1], STDOUT_FILENO);
        dup2(output_pipe[1], STDERR_FILENO);
        close(input_pipe[0]); close(input_pipe[1]);
        close(output_pipe[0]); close(output_pipe[1]);
        const std::string directory = executable_directory();
        chdir(directory.c_str());
        execl(backend.c_str(), backend.c_str(), "--gui", static_cast<char*>(nullptr));
        _exit(127);
    }

    close(input_pipe[0]);
    close(output_pipe[1]);
    g_child_pid = child;
    g_child_stdin = input_pipe[1];
    g_child_stdout = output_pipe[0];
    g_process_running = true;
    g_reader_thread = std::thread(reader_loop, g_child_stdout, child);
    set_status(T("Starting backend…"));
    return true;
}

bool require_ready() {
    if (g_ready) return true;
    show_message(GTK_MESSAGE_INFO, T("VeilKnit Daemon"), T("The daemon is not ready yet."));
    return false;
}

void restore_local_backup() {
    const std::string passphrase = entry_text(g_password_entry);
    if (passphrase.empty()) {
        show_message(GTK_MESSAGE_INFO, T("Restore backup"),
            T("Enter the backup passphrase in the Password field, then choose the backup file."));
        gtk_widget_grab_focus(g_password_entry);
        return;
    }
    GtkWidget* dialog = gtk_file_chooser_dialog_new(
        T("Restore backup"), GTK_WINDOW(g_window), GTK_FILE_CHOOSER_ACTION_OPEN,
        T("Cancel"), GTK_RESPONSE_CANCEL, T("Restore backup"), GTK_RESPONSE_ACCEPT, nullptr);
    GtkFileFilter* filter = gtk_file_filter_new();
    gtk_file_filter_set_name(filter, "VeilKnit backup (*.veilknit-backup)");
    gtk_file_filter_add_pattern(filter, "*.veilknit-backup");
    gtk_file_chooser_add_filter(GTK_FILE_CHOOSER(dialog), filter);
    if (gtk_dialog_run(GTK_DIALOG(dialog)) == GTK_RESPONSE_ACCEPT) {
        char* filename = gtk_file_chooser_get_filename(GTK_FILE_CHOOSER(dialog));
        if (filename && start_backend()) {
            enable_login(false);
            set_status(T("Restoring encrypted backup…"));
            send_lines({"r", filename, passphrase});
            gtk_entry_set_text(GTK_ENTRY(g_password_entry), "");
        }
        g_free(filename);
    }
    gtk_widget_destroy(dialog);
}

void open_discord() {
    GError* error = nullptr;
    if (!gtk_show_uri_on_window(GTK_WINDOW(g_window), "https://discord.gg/yy5SMTuZY",
                                GDK_CURRENT_TIME, &error)) {
        show_message(GTK_MESSAGE_ERROR, T("Discord"),
            error && error->message ? error->message : T("Could not open the external link."));
    }
    if (error) g_error_free(error);
}

void authenticate(bool signup) {
    const std::string username = entry_text(g_username_entry);
    const std::string password = entry_text(g_password_entry);
    if (username.empty() || password.empty()) return;
    if (!start_backend()) return;
    enable_login(false);
    send_lines({signup ? "s" : "l", username, password});
}

GtkWidget* make_scrolled_text_view(bool editable = false) {
    GtkWidget* scroll = gtk_scrolled_window_new(nullptr, nullptr);
    gtk_scrolled_window_set_policy(GTK_SCROLLED_WINDOW(scroll), GTK_POLICY_AUTOMATIC, GTK_POLICY_AUTOMATIC);
    GtkWidget* view = gtk_text_view_new();
    gtk_text_view_set_editable(GTK_TEXT_VIEW(view), editable);
    gtk_text_view_set_monospace(GTK_TEXT_VIEW(view), TRUE);
    gtk_text_view_set_wrap_mode(GTK_TEXT_VIEW(view), GTK_WRAP_WORD_CHAR);
    gtk_container_add(GTK_CONTAINER(scroll), view);
    g_object_set_data(G_OBJECT(scroll), "text-view", view);
    return scroll;
}

GtkWidget* labeled_spin(GtkWidget* grid, GtkWidget** label_out, GtkWidget** spin_out,
                        int row, const char* label, double min, double max, double value) {
    GtkWidget* text = gtk_label_new(label);
    gtk_widget_set_halign(text, GTK_ALIGN_START);
    GtkWidget* spin = gtk_spin_button_new_with_range(min, max, 1.0);
    gtk_spin_button_set_value(GTK_SPIN_BUTTON(spin), value);
    gtk_grid_attach(GTK_GRID(grid), text, 0, row, 1, 1);
    gtk_grid_attach(GTK_GRID(grid), spin, 1, row, 1, 1);
    *label_out = text;
    *spin_out = spin;
    return spin;
}

void set_tree_column_title(GtkWidget* view, int index, const char* english) {
    if (!view) return;
    GList* columns = gtk_tree_view_get_columns(GTK_TREE_VIEW(view));
    GtkTreeViewColumn* column = static_cast<GtkTreeViewColumn*>(g_list_nth_data(columns, index));
    if (column) gtk_tree_view_column_set_title(column, T(english));
    g_list_free(columns);
}

void apply_language() {
    gtk_window_set_title(GTK_WINDOW(g_window), T("VeilKnit Daemon"));
    gtk_label_set_text(GTK_LABEL(g_language_label), T("🌐 Language"));
    gtk_label_set_text(GTK_LABEL(g_username_label), T("Username"));
    gtk_label_set_text(GTK_LABEL(g_password_label), T("Password"));
    gtk_button_set_label(GTK_BUTTON(g_login_button), T("Log in"));
    gtk_button_set_label(GTK_BUTTON(g_signup_button), T("Create account"));
    gtk_button_set_label(GTK_BUTTON(g_restore_backup_button), T("Restore backup"));
    gtk_label_set_text(GTK_LABEL(g_status_caption), T("Status"));
    gtk_notebook_set_tab_label_text(GTK_NOTEBOOK(g_notebook), g_tab_overview, T("Overview"));
    gtk_notebook_set_tab_label_text(GTK_NOTEBOOK(g_notebook), g_tab_network, T("Network"));
    gtk_notebook_set_tab_label_text(GTK_NOTEBOOK(g_notebook), g_tab_headers, T("Headers"));
    gtk_notebook_set_tab_label_text(GTK_NOTEBOOK(g_notebook), g_tab_apps, T("Applications"));
    gtk_notebook_set_tab_label_text(GTK_NOTEBOOK(g_notebook), g_tab_logs, T("All logs"));
    gtk_label_set_text(GTK_LABEL(g_main_key_label), T("Main DHT key"));
    gtk_button_set_label(GTK_BUTTON(g_copy_key_button), T("Copy key"));
    gtk_button_set_label(GTK_BUTTON(g_help_button), T("Help"));
    gtk_button_set_label(GTK_BUTTON(g_discord_button), T("Discord"));
    gtk_button_set_label(GTK_BUTTON(g_save_log_button), T("Save log"));
    gtk_button_set_label(GTK_BUTTON(g_stop_button), T("Stop safely"));
    gtk_frame_set_label(GTK_FRAME(g_normal_frame), T("Normal walking mode"));
    gtk_frame_set_label(GTK_FRAME(g_mail_frame), T("Mail walking mode"));
    gtk_label_set_text(GTK_LABEL(g_normal_min_hops_label), T("Minimum hops"));
    gtk_label_set_text(GTK_LABEL(g_normal_max_hops_label), T("Maximum hops"));
    gtk_label_set_text(GTK_LABEL(g_normal_min_sec_label), T("Minimum interval (seconds)"));
    gtk_label_set_text(GTK_LABEL(g_normal_target_sec_label), T("Target interval (seconds)"));
    gtk_label_set_text(GTK_LABEL(g_normal_max_sec_label), T("Maximum interval (seconds)"));
    gtk_label_set_text(GTK_LABEL(g_mail_min_hops_label), T("Minimum hops"));
    gtk_label_set_text(GTK_LABEL(g_mail_max_hops_label), T("Maximum hops"));
    gtk_label_set_text(GTK_LABEL(g_mail_min_sec_label), T("Minimum interval (seconds)"));
    gtk_label_set_text(GTK_LABEL(g_mail_target_sec_label), T("Target interval (seconds)"));
    gtk_label_set_text(GTK_LABEL(g_mail_max_sec_label), T("Maximum interval (seconds)"));
    gtk_button_set_label(GTK_BUTTON(g_mail_auto_check), T("Use mail mode for automatic walks"));
    gtk_button_set_label(GTK_BUTTON(g_apply_walk_button), T("Apply and save walk settings"));
    gtk_button_set_label(GTK_BUTTON(g_start_normal_button), T("Start normal walk"));
    gtk_button_set_label(GTK_BUTTON(g_start_mail_button), T("Start mail walk"));
    gtk_button_set_label(GTK_BUTTON(g_walk_status_button), T("Walk status"));
    gtk_button_set_label(GTK_BUTTON(g_stop_walk_button), T("Stop walk"));
    gtk_button_set_label(GTK_BUTTON(g_route_status_button), T("Route status"));
    gtk_button_set_label(GTK_BUTTON(g_node_list_button), T("Node list"));
    gtk_button_set_label(GTK_BUTTON(g_daemon_status_button), T("Daemon status"));
    gtk_frame_set_label(GTK_FRAME(g_summary_topology_frame), T("Topology"));
    gtk_frame_set_label(GTK_FRAME(g_summary_presence_frame), T("Presence"));
    gtk_frame_set_label(GTK_FRAME(g_summary_headers_frame), T("Header cache"));
    gtk_frame_set_label(GTK_FRAME(g_summary_activity_frame), T("Activity"));
    gtk_label_set_text(GTK_LABEL(g_main_header_label), T("Published main/presence header (subkey 0)"));
    gtk_button_set_label(GTK_BUTTON(g_copy_main_header_button), T("Copy main header"));
    gtk_label_set_text(GTK_LABEL(g_mail_header_label), T("Published mailbox advertisement (subkey 2)"));
    gtk_button_set_label(GTK_BUTTON(g_copy_mail_header_button), T("Copy mailbox header"));
    gtk_button_set_label(GTK_BUTTON(g_refresh_headers_button), T("Refresh both headers"));
    if (g_apps_advanced_expander) gtk_expander_set_label(GTK_EXPANDER(g_apps_advanced_expander), T("Advanced application management"));
    gtk_frame_set_label(GTK_FRAME(g_apps_frame), T("Local applications"));
    gtk_label_set_text(GTK_LABEL(g_app_id_label), T("Application id"));
    gtk_label_set_text(GTK_LABEL(g_app_name_label), T("Display name"));
    gtk_button_set_label(GTK_BUTTON(g_app_register_button), T("Register"));
    gtk_button_set_label(GTK_BUTTON(g_app_list_button), T("List"));
    gtk_button_set_label(GTK_BUTTON(g_app_rotate_button), T("Rotate selected app key"));
    gtk_frame_set_label(GTK_FRAME(g_requests_frame), T("Registration requests"));
    gtk_label_set_text(GTK_LABEL(g_app_help_label), T("Newest authorization request per application is shown. Check the applications you want, then allow or refuse the checked rows."));
    gtk_button_set_label(GTK_BUTTON(g_app_pending_button), T("Refresh requests"));
    gtk_label_set_text(GTK_LABEL(g_request_id_label), T("Request id"));
    gtk_label_set_text(GTK_LABEL(g_reject_reason_label), T("Refusal reason"));
    gtk_button_set_label(GTK_BUTTON(g_approve_button), T("Allow checked"));
    gtk_button_set_label(GTK_BUTTON(g_reject_button), T("Refuse checked"));
    if (g_found_apps_frame) gtk_frame_set_label(GTK_FRAME(g_found_apps_frame), T("Observed applications"));
    set_tree_column_title(g_request_view, 1, "Application");
    set_tree_column_title(g_request_view, 2, "Display name");
    set_tree_column_title(g_request_view, 3, "Request");
    set_tree_column_title(g_found_app_view, 0, "Application");
    set_tree_column_title(g_found_app_view, 1, "Verified headers");
    set_tree_column_title(g_found_app_view, 2, "Discovery cache");
    set_tree_column_title(g_found_app_view, 3, "Recent");
    set_tree_column_title(g_found_app_view, 4, "Archive");
    gtk_frame_set_label(GTK_FRAME(g_visible_names_frame), T("Names shown to applications"));
    gtk_label_set_text(GTK_LABEL(g_visible_name_label), T("Visible name"));
    gtk_button_set_label(GTK_BUTTON(g_set_default_visible_name_button), T("Set default visible name"));
    gtk_button_set_label(GTK_BUTTON(g_set_app_alias_button), T("Set selected app alias"));
    gtk_button_set_label(GTK_BUTTON(g_clear_app_alias_button), T("Clear selected alias"));
    gtk_button_set_label(GTK_BUTTON(g_list_visible_names_button), T("List name settings"));
    gtk_frame_set_label(GTK_FRAME(g_profiles_frame), T("Network profiles"));
    gtk_label_set_text(GTK_LABEL(g_profile_name_label), T("New profile name"));
    gtk_label_set_text(GTK_LABEL(g_profile_id_label), T("Profile id"));
    gtk_button_set_label(GTK_BUTTON(g_profile_create_button), T("Create profile"));
    gtk_button_set_label(GTK_BUTTON(g_profile_list_button), T("List profiles"));
    gtk_button_set_label(GTK_BUTTON(g_profile_use_button), T("Use after restart"));
    gtk_button_set_label(GTK_BUTTON(g_profile_retire_button), T("Retire profile"));
    gtk_label_set_text(GTK_LABEL(g_profile_restart_note), T("Profile changes take effect after a safe daemon restart."));
}

void language_changed(GtkComboBox* combo, gpointer) {
    const gint index = gtk_combo_box_get_active(combo);
    if (index < 0 || index > 4) return;
    g_language = static_cast<UiLanguage>(index);
    save_language();
    apply_language();
}

void save_log() {
    GtkWidget* dialog = gtk_file_chooser_dialog_new(T("Save log"), GTK_WINDOW(g_window),
        GTK_FILE_CHOOSER_ACTION_SAVE, T("Cancel"), GTK_RESPONSE_CANCEL,
        T("Save log"), GTK_RESPONSE_ACCEPT, nullptr);
    gtk_file_chooser_set_current_name(GTK_FILE_CHOOSER(dialog), "veilknit-session.log");
    if (gtk_dialog_run(GTK_DIALOG(dialog)) == GTK_RESPONSE_ACCEPT) {
        char* filename = gtk_file_chooser_get_filename(GTK_FILE_CHOOSER(dialog));
        if (filename) {
            std::ofstream output(filename, std::ios::binary | std::ios::trunc);
            output << text_view_text(g_log_view);
            g_free(filename);
        }
    }
    gtk_widget_destroy(dialog);
}

void show_help() {
    show_message(GTK_MESSAGE_INFO, T("Getting started"),
        T("Enter a username and password, then log in or create an account. To restore a local backup, put its backup passphrase in the Password field and press Restore backup. To link an app, open it once, go to Applications, show pending requests, enter the request number, and approve it. Network contains separate normal and mail walk controls. Headers displays the published presence and mailbox records. Community support is available through the Discord button; Discord is an external service and is not required to use VeilKnit."));
}

void apply_walk_settings() {
    if (!require_ready()) return;
    GtkWidget* widgets[] = {g_normal_min_hops, g_normal_max_hops, g_normal_min_sec,
        g_normal_target_sec, g_normal_max_sec, g_mail_min_hops, g_mail_max_hops,
        g_mail_min_sec, g_mail_target_sec, g_mail_max_sec};
    std::ostringstream command;
    command << "walk-set";
    for (auto* widget : widgets) command << ' ' << gtk_spin_button_get_value_as_int(GTK_SPIN_BUTTON(widget));
    command << ' ' << (gtk_toggle_button_get_active(GTK_TOGGLE_BUTTON(g_mail_auto_check)) ? 1 : 0);
    send_line(command.str());
    set_status(T("Settings sent"));
}

void stop_backend() {
    if (g_process_running) send_line("Q");
}

void window_destroy(GtkWidget*, gpointer) {
    if (g_process_running) {
        if (g_authenticated) send_line("Q");
        else if (g_child_pid > 0) kill(g_child_pid, SIGTERM);
    }
    gtk_main_quit();
}

void build_ui() {
    g_window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_default_size(GTK_WINDOW(g_window), 1050, 760);
    gtk_container_set_border_width(GTK_CONTAINER(g_window), 8);
    g_signal_connect(g_window, "destroy", G_CALLBACK(window_destroy), nullptr);

    GtkWidget* outer = gtk_box_new(GTK_ORIENTATION_VERTICAL, 8);
    gtk_container_add(GTK_CONTAINER(g_window), outer);

    g_login_frame = gtk_frame_new(nullptr);
    GtkWidget* login_grid = gtk_grid_new();
    gtk_grid_set_row_spacing(GTK_GRID(login_grid), 6);
    gtk_grid_set_column_spacing(GTK_GRID(login_grid), 8);
    gtk_container_set_border_width(GTK_CONTAINER(login_grid), 8);
    gtk_container_add(GTK_CONTAINER(g_login_frame), login_grid);
    gtk_box_pack_start(GTK_BOX(outer), g_login_frame, FALSE, FALSE, 0);

    g_language_label = gtk_label_new("");
    g_language_combo = gtk_combo_box_text_new();
    for (UiLanguage language : {UiLanguage::English, UiLanguage::French, UiLanguage::Spanish, UiLanguage::Russian, UiLanguage::Chinese})
        gtk_combo_box_text_append_text(GTK_COMBO_BOX_TEXT(g_language_combo), language_native_name(language));
    gtk_combo_box_set_active(GTK_COMBO_BOX(g_language_combo), static_cast<int>(g_language));
    g_username_label = gtk_label_new("");
    g_username_entry = gtk_entry_new();
    g_password_label = gtk_label_new("");
    g_password_entry = gtk_entry_new();
    gtk_entry_set_visibility(GTK_ENTRY(g_password_entry), FALSE);
    gtk_entry_set_input_purpose(GTK_ENTRY(g_password_entry), GTK_INPUT_PURPOSE_PASSWORD);
    g_login_button = gtk_button_new();
    g_signup_button = gtk_button_new();
    g_restore_backup_button = gtk_button_new();
    g_status_caption = gtk_label_new("");
    g_status_label = gtk_label_new("");
    gtk_widget_set_halign(g_status_label, GTK_ALIGN_START);

    gtk_grid_attach(GTK_GRID(login_grid), g_language_label, 0, 0, 1, 1);
    gtk_grid_attach(GTK_GRID(login_grid), g_language_combo, 1, 0, 1, 1);
    gtk_grid_attach(GTK_GRID(login_grid), g_status_caption, 2, 0, 1, 1);
    gtk_grid_attach(GTK_GRID(login_grid), g_status_label, 3, 0, 3, 1);
    gtk_grid_attach(GTK_GRID(login_grid), g_username_label, 0, 1, 1, 1);
    gtk_grid_attach(GTK_GRID(login_grid), g_username_entry, 1, 1, 2, 1);
    gtk_grid_attach(GTK_GRID(login_grid), g_password_label, 3, 1, 1, 1);
    gtk_grid_attach(GTK_GRID(login_grid), g_password_entry, 4, 1, 1, 1);
    gtk_grid_attach(GTK_GRID(login_grid), g_login_button, 5, 1, 1, 1);
    gtk_grid_attach(GTK_GRID(login_grid), g_signup_button, 6, 1, 1, 1);
    gtk_grid_attach(GTK_GRID(login_grid), g_restore_backup_button, 7, 1, 1, 1);

    g_notebook = gtk_notebook_new();
    gtk_box_pack_start(GTK_BOX(outer), g_notebook, TRUE, TRUE, 0);

    // Overview
    g_tab_overview = gtk_box_new(GTK_ORIENTATION_VERTICAL, 8);
    gtk_container_set_border_width(GTK_CONTAINER(g_tab_overview), 10);
    GtkWidget* key_grid = gtk_grid_new();
    gtk_grid_set_column_spacing(GTK_GRID(key_grid), 8);
    g_main_key_label = gtk_label_new("");
    g_main_key_entry = gtk_entry_new();
    gtk_editable_set_editable(GTK_EDITABLE(g_main_key_entry), FALSE);
    gtk_widget_set_hexpand(g_main_key_entry, TRUE);
    g_copy_key_button = gtk_button_new();
    gtk_grid_attach(GTK_GRID(key_grid), g_main_key_label, 0, 0, 1, 1);
    gtk_grid_attach(GTK_GRID(key_grid), g_main_key_entry, 1, 0, 1, 1);
    gtk_grid_attach(GTK_GRID(key_grid), g_copy_key_button, 2, 0, 1, 1);
    gtk_box_pack_start(GTK_BOX(g_tab_overview), key_grid, FALSE, FALSE, 0);
    GtkWidget* overview_buttons = gtk_button_box_new(GTK_ORIENTATION_HORIZONTAL);
    gtk_button_box_set_layout(GTK_BUTTON_BOX(overview_buttons), GTK_BUTTONBOX_START);
    g_help_button = gtk_button_new();
    g_discord_button = gtk_button_new();
    g_save_log_button = gtk_button_new();
    g_stop_button = gtk_button_new();
    gtk_container_add(GTK_CONTAINER(overview_buttons), g_help_button);
    gtk_container_add(GTK_CONTAINER(overview_buttons), g_discord_button);
    gtk_container_add(GTK_CONTAINER(overview_buttons), g_save_log_button);
    gtk_container_add(GTK_CONTAINER(overview_buttons), g_stop_button);
    gtk_box_pack_start(GTK_BOX(g_tab_overview), overview_buttons, FALSE, FALSE, 0);
    gtk_notebook_append_page(GTK_NOTEBOOK(g_notebook), g_tab_overview, gtk_label_new(""));

    // Network
    g_tab_network = gtk_box_new(GTK_ORIENTATION_VERTICAL, 8);
    gtk_container_set_border_width(GTK_CONTAINER(g_tab_network), 10);

    GtkWidget* summary_grid = gtk_grid_new();
    gtk_grid_set_row_spacing(GTK_GRID(summary_grid), 8);
    gtk_grid_set_column_spacing(GTK_GRID(summary_grid), 8);
    gtk_grid_set_column_homogeneous(GTK_GRID(summary_grid), TRUE);
    g_summary_topology_frame = gtk_frame_new("");
    g_summary_presence_frame = gtk_frame_new("");
    g_summary_headers_frame = gtk_frame_new("");
    g_summary_activity_frame = gtk_frame_new("");
    g_summary_topology_label = gtk_label_new("Verified: 0\nCandidates: 0\nAuthenticated: 0");
    g_summary_presence_label = gtk_label_new("Online: 0\nOffline: 0\nStale claim: 0\nNeeds refresh: 0\nUnknown: 0");
    g_summary_headers_label = gtk_label_new("Presence OK: 0\nRead failed: 0\nNot checked: 0\nActive app info: 0\nMailbox capable: 0");
    g_summary_activity_label = gtk_label_new("Walk: idle\nNew / updated: 0 / 0\nReach / fail: 0 / 0\nApp searches: 0\nRoot lookups: 0");
    for (GtkWidget* label : {g_summary_topology_label, g_summary_presence_label,
         g_summary_headers_label, g_summary_activity_label}) {
        gtk_widget_set_halign(label, GTK_ALIGN_START);
        gtk_widget_set_margin_start(label, 8);
        gtk_widget_set_margin_end(label, 8);
        gtk_widget_set_margin_top(label, 6);
        gtk_widget_set_margin_bottom(label, 8);
    }
    gtk_container_add(GTK_CONTAINER(g_summary_topology_frame), g_summary_topology_label);
    gtk_container_add(GTK_CONTAINER(g_summary_presence_frame), g_summary_presence_label);
    gtk_container_add(GTK_CONTAINER(g_summary_headers_frame), g_summary_headers_label);
    gtk_container_add(GTK_CONTAINER(g_summary_activity_frame), g_summary_activity_label);
    gtk_grid_attach(GTK_GRID(summary_grid), g_summary_topology_frame, 0, 0, 1, 1);
    gtk_grid_attach(GTK_GRID(summary_grid), g_summary_presence_frame, 1, 0, 1, 1);
    gtk_grid_attach(GTK_GRID(summary_grid), g_summary_headers_frame, 0, 1, 1, 1);
    gtk_grid_attach(GTK_GRID(summary_grid), g_summary_activity_frame, 1, 1, 1, 1);
    gtk_box_pack_start(GTK_BOX(g_tab_network), summary_grid, FALSE, FALSE, 0);

    GtkWidget* modes = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 10);
    g_normal_frame = gtk_frame_new("");
    g_mail_frame = gtk_frame_new("");
    GtkWidget* normal_grid = gtk_grid_new();
    GtkWidget* mail_grid = gtk_grid_new();
    gtk_grid_set_row_spacing(GTK_GRID(normal_grid), 5); gtk_grid_set_column_spacing(GTK_GRID(normal_grid), 8);
    gtk_grid_set_row_spacing(GTK_GRID(mail_grid), 5); gtk_grid_set_column_spacing(GTK_GRID(mail_grid), 8);
    gtk_container_set_border_width(GTK_CONTAINER(normal_grid), 8);
    gtk_container_set_border_width(GTK_CONTAINER(mail_grid), 8);
    gtk_container_add(GTK_CONTAINER(g_normal_frame), normal_grid);
    gtk_container_add(GTK_CONTAINER(g_mail_frame), mail_grid);
    labeled_spin(normal_grid, &g_normal_min_hops_label, &g_normal_min_hops, 0, "", 1, 250, 4);
    labeled_spin(normal_grid, &g_normal_max_hops_label, &g_normal_max_hops, 1, "", 1, 250, 20);
    labeled_spin(normal_grid, &g_normal_min_sec_label, &g_normal_min_sec, 2, "", 1, 86400, 120);
    labeled_spin(normal_grid, &g_normal_target_sec_label, &g_normal_target_sec, 3, "", 1, 86400, 300);
    labeled_spin(normal_grid, &g_normal_max_sec_label, &g_normal_max_sec, 4, "", 1, 86400, 600);
    labeled_spin(mail_grid, &g_mail_min_hops_label, &g_mail_min_hops, 0, "", 1, 250, 6);
    labeled_spin(mail_grid, &g_mail_max_hops_label, &g_mail_max_hops, 1, "", 1, 250, 30);
    labeled_spin(mail_grid, &g_mail_min_sec_label, &g_mail_min_sec, 2, "", 1, 86400, 120);
    labeled_spin(mail_grid, &g_mail_target_sec_label, &g_mail_target_sec, 3, "", 1, 86400, 180);
    labeled_spin(mail_grid, &g_mail_max_sec_label, &g_mail_max_sec, 4, "", 1, 86400, 600);
    gtk_box_pack_start(GTK_BOX(modes), g_normal_frame, TRUE, TRUE, 0);
    gtk_box_pack_start(GTK_BOX(modes), g_mail_frame, TRUE, TRUE, 0);
    gtk_box_pack_start(GTK_BOX(g_tab_network), modes, FALSE, FALSE, 0);
    g_mail_auto_check = gtk_check_button_new();
    gtk_box_pack_start(GTK_BOX(g_tab_network), g_mail_auto_check, FALSE, FALSE, 0);
    GtkWidget* network_buttons = gtk_flow_box_new();
    gtk_flow_box_set_selection_mode(GTK_FLOW_BOX(network_buttons), GTK_SELECTION_NONE);
    g_apply_walk_button = gtk_button_new();
    g_start_normal_button = gtk_button_new();
    g_start_mail_button = gtk_button_new();
    g_walk_status_button = gtk_button_new();
    g_stop_walk_button = gtk_button_new();
    g_route_status_button = gtk_button_new();
    g_node_list_button = gtk_button_new();
    g_daemon_status_button = gtk_button_new();
    for (GtkWidget* button : {g_apply_walk_button, g_start_normal_button, g_start_mail_button,
        g_walk_status_button, g_stop_walk_button, g_route_status_button, g_node_list_button, g_daemon_status_button})
        gtk_container_add(GTK_CONTAINER(network_buttons), button);
    gtk_box_pack_start(GTK_BOX(g_tab_network), network_buttons, FALSE, FALSE, 0);
    gtk_notebook_append_page(GTK_NOTEBOOK(g_notebook), g_tab_network, gtk_label_new(""));

    // Headers
    g_tab_headers = gtk_box_new(GTK_ORIENTATION_VERTICAL, 6);
    gtk_container_set_border_width(GTK_CONTAINER(g_tab_headers), 10);
    g_main_header_label = gtk_label_new(""); gtk_widget_set_halign(g_main_header_label, GTK_ALIGN_START);
    GtkWidget* main_scroll = make_scrolled_text_view();
    g_main_header_view = static_cast<GtkWidget*>(g_object_get_data(G_OBJECT(main_scroll), "text-view"));
    g_copy_main_header_button = gtk_button_new();
    g_mail_header_label = gtk_label_new(""); gtk_widget_set_halign(g_mail_header_label, GTK_ALIGN_START);
    GtkWidget* mail_scroll = make_scrolled_text_view();
    g_mail_header_view = static_cast<GtkWidget*>(g_object_get_data(G_OBJECT(mail_scroll), "text-view"));
    g_copy_mail_header_button = gtk_button_new();
    g_refresh_headers_button = gtk_button_new();
    gtk_box_pack_start(GTK_BOX(g_tab_headers), g_main_header_label, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(g_tab_headers), main_scroll, TRUE, TRUE, 0);
    gtk_box_pack_start(GTK_BOX(g_tab_headers), g_copy_main_header_button, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(g_tab_headers), g_mail_header_label, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(g_tab_headers), mail_scroll, TRUE, TRUE, 0);
    GtkWidget* header_buttons = gtk_button_box_new(GTK_ORIENTATION_HORIZONTAL);
    gtk_container_add(GTK_CONTAINER(header_buttons), g_copy_mail_header_button);
    gtk_container_add(GTK_CONTAINER(header_buttons), g_refresh_headers_button);
    gtk_box_pack_start(GTK_BOX(g_tab_headers), header_buttons, FALSE, FALSE, 0);
    gtk_notebook_append_page(GTK_NOTEBOOK(g_notebook), g_tab_headers, gtk_label_new(""));

    // Applications
    g_tab_apps = gtk_box_new(GTK_ORIENTATION_VERTICAL, 8);
    gtk_container_set_border_width(GTK_CONTAINER(g_tab_apps), 10);
    g_apps_frame = gtk_frame_new("");
    GtkWidget* app_grid = gtk_grid_new();
    gtk_grid_set_row_spacing(GTK_GRID(app_grid), 6); gtk_grid_set_column_spacing(GTK_GRID(app_grid), 8);
    gtk_container_set_border_width(GTK_CONTAINER(app_grid), 8);
    gtk_container_add(GTK_CONTAINER(g_apps_frame), app_grid);
    g_app_id_label = gtk_label_new(""); g_app_id_entry = gtk_entry_new();
    g_app_name_label = gtk_label_new(""); g_app_name_entry = gtk_entry_new();
    g_app_register_button = gtk_button_new(); g_app_list_button = gtk_button_new(); g_app_rotate_button = gtk_button_new();
    gtk_grid_attach(GTK_GRID(app_grid), g_app_id_label, 0, 0, 1, 1);
    gtk_grid_attach(GTK_GRID(app_grid), g_app_id_entry, 1, 0, 2, 1);
    gtk_grid_attach(GTK_GRID(app_grid), g_app_name_label, 0, 1, 1, 1);
    gtk_grid_attach(GTK_GRID(app_grid), g_app_name_entry, 1, 1, 2, 1);
    gtk_grid_attach(GTK_GRID(app_grid), g_app_register_button, 0, 2, 1, 1);
    gtk_grid_attach(GTK_GRID(app_grid), g_app_list_button, 1, 2, 1, 1);
    gtk_grid_attach(GTK_GRID(app_grid), g_app_rotate_button, 2, 2, 1, 1);
    g_requests_frame = gtk_frame_new("");
    GtkWidget* request_grid = gtk_grid_new();
    gtk_grid_set_row_spacing(GTK_GRID(request_grid), 6); gtk_grid_set_column_spacing(GTK_GRID(request_grid), 8);
    gtk_container_set_border_width(GTK_CONTAINER(request_grid), 8);
    gtk_container_add(GTK_CONTAINER(g_requests_frame), request_grid);
    g_app_help_label = gtk_label_new(""); gtk_label_set_line_wrap(GTK_LABEL(g_app_help_label), TRUE); gtk_widget_set_halign(g_app_help_label, GTK_ALIGN_START);
    g_app_pending_button = gtk_button_new();
    g_request_id_label = gtk_label_new(""); g_request_id_entry = gtk_entry_new();
    g_reject_reason_label = gtk_label_new(""); g_reject_reason_entry = gtk_entry_new();
    g_approve_button = gtk_button_new(); g_reject_button = gtk_button_new();
    g_request_store = gtk_list_store_new(REQ_COLUMN_COUNT, G_TYPE_BOOLEAN, G_TYPE_STRING, G_TYPE_STRING, G_TYPE_UINT64);
    g_request_view = gtk_tree_view_new_with_model(GTK_TREE_MODEL(g_request_store));
    GtkCellRenderer* request_toggle_renderer = gtk_cell_renderer_toggle_new();
    g_signal_connect(request_toggle_renderer, "toggled", G_CALLBACK(request_toggle), nullptr);
    gtk_tree_view_append_column(GTK_TREE_VIEW(g_request_view), gtk_tree_view_column_new_with_attributes("", request_toggle_renderer, "active", REQ_CHECKED, nullptr));
    gtk_tree_view_append_column(GTK_TREE_VIEW(g_request_view), gtk_tree_view_column_new_with_attributes(T("Application"), gtk_cell_renderer_text_new(), "text", REQ_APP, nullptr));
    gtk_tree_view_append_column(GTK_TREE_VIEW(g_request_view), gtk_tree_view_column_new_with_attributes(T("Display name"), gtk_cell_renderer_text_new(), "text", REQ_NAME, nullptr));
    gtk_tree_view_append_column(GTK_TREE_VIEW(g_request_view), gtk_tree_view_column_new_with_attributes(T("Request"), gtk_cell_renderer_text_new(), "text", REQ_ID, nullptr));
    GtkWidget* request_scroll = gtk_scrolled_window_new(nullptr, nullptr);
    gtk_scrolled_window_set_policy(GTK_SCROLLED_WINDOW(request_scroll), GTK_POLICY_AUTOMATIC, GTK_POLICY_AUTOMATIC);
    gtk_widget_set_size_request(request_scroll, -1, 125);
    gtk_container_add(GTK_CONTAINER(request_scroll), g_request_view);
    gtk_grid_attach(GTK_GRID(request_grid), g_app_help_label, 0, 0, 4, 1);
    gtk_grid_attach(GTK_GRID(request_grid), g_app_pending_button, 0, 1, 1, 1);
    gtk_grid_attach(GTK_GRID(request_grid), g_approve_button, 1, 1, 1, 1);
    gtk_grid_attach(GTK_GRID(request_grid), g_reject_button, 2, 1, 1, 1);
    gtk_grid_attach(GTK_GRID(request_grid), request_scroll, 0, 2, 4, 1);
    gtk_box_pack_start(GTK_BOX(g_tab_apps), g_requests_frame, FALSE, FALSE, 0);

    g_found_apps_frame = gtk_frame_new("");
    g_found_app_store = gtk_list_store_new(FOUND_COLUMN_COUNT, G_TYPE_STRING, G_TYPE_UINT, G_TYPE_UINT, G_TYPE_UINT, G_TYPE_UINT);
    g_found_app_view = gtk_tree_view_new_with_model(GTK_TREE_MODEL(g_found_app_store));
    gtk_tree_view_append_column(GTK_TREE_VIEW(g_found_app_view), gtk_tree_view_column_new_with_attributes(T("Application"), gtk_cell_renderer_text_new(), "text", FOUND_APP, nullptr));
    gtk_tree_view_append_column(GTK_TREE_VIEW(g_found_app_view), gtk_tree_view_column_new_with_attributes(T("Verified headers"), gtk_cell_renderer_text_new(), "text", FOUND_OBSERVED, nullptr));
    gtk_tree_view_append_column(GTK_TREE_VIEW(g_found_app_view), gtk_tree_view_column_new_with_attributes(T("Discovery cache"), gtk_cell_renderer_text_new(), "text", FOUND_CACHED, nullptr));
    gtk_tree_view_append_column(GTK_TREE_VIEW(g_found_app_view), gtk_tree_view_column_new_with_attributes(T("Recent"), gtk_cell_renderer_text_new(), "text", FOUND_RECENT, nullptr));
    gtk_tree_view_append_column(GTK_TREE_VIEW(g_found_app_view), gtk_tree_view_column_new_with_attributes(T("Archive"), gtk_cell_renderer_text_new(), "text", FOUND_ARCHIVE, nullptr));
    GtkWidget* found_scroll = gtk_scrolled_window_new(nullptr, nullptr);
    gtk_scrolled_window_set_policy(GTK_SCROLLED_WINDOW(found_scroll), GTK_POLICY_AUTOMATIC, GTK_POLICY_AUTOMATIC);
    gtk_widget_set_size_request(found_scroll, -1, 125);
    gtk_container_set_border_width(GTK_CONTAINER(found_scroll), 6);
    gtk_container_add(GTK_CONTAINER(found_scroll), g_found_app_view);
    gtk_container_add(GTK_CONTAINER(g_found_apps_frame), found_scroll);
    gtk_box_pack_start(GTK_BOX(g_tab_apps), g_found_apps_frame, FALSE, FALSE, 0);

    g_visible_names_frame = gtk_frame_new("");
    GtkWidget* visible_grid = gtk_grid_new();
    gtk_grid_set_row_spacing(GTK_GRID(visible_grid), 6);
    gtk_grid_set_column_spacing(GTK_GRID(visible_grid), 8);
    gtk_container_set_border_width(GTK_CONTAINER(visible_grid), 8);
    gtk_container_add(GTK_CONTAINER(g_visible_names_frame), visible_grid);
    g_visible_name_label = gtk_label_new("");
    g_visible_name_entry = gtk_entry_new();
    g_set_default_visible_name_button = gtk_button_new();
    g_set_app_alias_button = gtk_button_new();
    g_clear_app_alias_button = gtk_button_new();
    g_list_visible_names_button = gtk_button_new();
    gtk_grid_attach(GTK_GRID(visible_grid), g_visible_name_label, 0, 0, 1, 1);
    gtk_grid_attach(GTK_GRID(visible_grid), g_visible_name_entry, 1, 0, 3, 1);
    gtk_grid_attach(GTK_GRID(visible_grid), g_set_default_visible_name_button, 0, 1, 1, 1);
    gtk_grid_attach(GTK_GRID(visible_grid), g_set_app_alias_button, 1, 1, 1, 1);
    gtk_grid_attach(GTK_GRID(visible_grid), g_clear_app_alias_button, 2, 1, 1, 1);
    gtk_grid_attach(GTK_GRID(visible_grid), g_list_visible_names_button, 3, 1, 1, 1);

    g_profiles_frame = gtk_frame_new("");
    GtkWidget* profile_grid = gtk_grid_new();
    gtk_grid_set_row_spacing(GTK_GRID(profile_grid), 6);
    gtk_grid_set_column_spacing(GTK_GRID(profile_grid), 8);
    gtk_container_set_border_width(GTK_CONTAINER(profile_grid), 8);
    gtk_container_add(GTK_CONTAINER(g_profiles_frame), profile_grid);
    g_profile_name_label = gtk_label_new("");
    g_profile_name_entry = gtk_entry_new();
    g_profile_id_label = gtk_label_new("");
    g_profile_id_entry = gtk_entry_new();
    g_profile_create_button = gtk_button_new();
    g_profile_list_button = gtk_button_new();
    g_profile_use_button = gtk_button_new();
    g_profile_retire_button = gtk_button_new();
    g_profile_restart_note = gtk_label_new("");
    gtk_label_set_line_wrap(GTK_LABEL(g_profile_restart_note), TRUE);
    gtk_widget_set_halign(g_profile_restart_note, GTK_ALIGN_START);
    gtk_grid_attach(GTK_GRID(profile_grid), g_profile_name_label, 0, 0, 1, 1);
    gtk_grid_attach(GTK_GRID(profile_grid), g_profile_name_entry, 1, 0, 3, 1);
    gtk_grid_attach(GTK_GRID(profile_grid), g_profile_create_button, 0, 1, 2, 1);
    gtk_grid_attach(GTK_GRID(profile_grid), g_profile_list_button, 2, 1, 2, 1);
    gtk_grid_attach(GTK_GRID(profile_grid), g_profile_id_label, 0, 2, 1, 1);
    gtk_grid_attach(GTK_GRID(profile_grid), g_profile_id_entry, 1, 2, 3, 1);
    gtk_grid_attach(GTK_GRID(profile_grid), g_profile_use_button, 0, 3, 2, 1);
    gtk_grid_attach(GTK_GRID(profile_grid), g_profile_retire_button, 2, 3, 2, 1);
    gtk_grid_attach(GTK_GRID(profile_grid), g_profile_restart_note, 0, 4, 4, 1);
    // Less common registration, rejection, alias and profile controls remain
    // available without crowding the normal request-number approval path.
    g_apps_advanced_expander = gtk_expander_new("");
    GtkWidget* advanced_box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 8);
    gtk_container_set_border_width(GTK_CONTAINER(advanced_box), 6);
    gtk_box_pack_start(GTK_BOX(advanced_box), g_apps_frame, FALSE, FALSE, 0);
    GtkWidget* reject_grid = gtk_grid_new();
    gtk_grid_set_column_spacing(GTK_GRID(reject_grid), 8);
    gtk_grid_attach(GTK_GRID(reject_grid), g_reject_reason_label, 0, 0, 1, 1);
    gtk_grid_attach(GTK_GRID(reject_grid), g_reject_reason_entry, 1, 0, 2, 1);
    gtk_box_pack_start(GTK_BOX(advanced_box), reject_grid, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(advanced_box), g_visible_names_frame, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(advanced_box), g_profiles_frame, FALSE, FALSE, 0);
    gtk_container_add(GTK_CONTAINER(g_apps_advanced_expander), advanced_box);
    gtk_box_pack_start(GTK_BOX(g_tab_apps), g_apps_advanced_expander, FALSE, FALSE, 0);

    gtk_notebook_append_page(GTK_NOTEBOOK(g_notebook), g_tab_apps, gtk_label_new(""));

    // Logs
    g_tab_logs = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    GtkWidget* log_scroll = make_scrolled_text_view();
    g_log_view = static_cast<GtkWidget*>(g_object_get_data(G_OBJECT(log_scroll), "text-view"));
    gtk_box_pack_start(GTK_BOX(g_tab_logs), log_scroll, TRUE, TRUE, 0);
    gtk_notebook_append_page(GTK_NOTEBOOK(g_notebook), g_tab_logs, gtk_label_new(""));

    // Signals
    g_signal_connect(g_language_combo, "changed", G_CALLBACK(language_changed), nullptr);
    g_signal_connect_swapped(g_login_button, "clicked", G_CALLBACK((+[](gpointer){ authenticate(false); })), nullptr);
    g_signal_connect_swapped(g_signup_button, "clicked", G_CALLBACK((+[](gpointer){ authenticate(true); })), nullptr);
    g_signal_connect_swapped(g_restore_backup_button, "clicked", G_CALLBACK((+[](gpointer){ restore_local_backup(); })), nullptr);
    g_signal_connect_swapped(g_copy_key_button, "clicked", G_CALLBACK((+[](gpointer){ copy_text(entry_text(g_main_key_entry)); })), nullptr);
    g_signal_connect_swapped(g_help_button, "clicked", G_CALLBACK((+[](gpointer){ show_help(); })), nullptr);
    g_signal_connect_swapped(g_discord_button, "clicked", G_CALLBACK((+[](gpointer){ open_discord(); })), nullptr);
    g_signal_connect_swapped(g_save_log_button, "clicked", G_CALLBACK((+[](gpointer){ save_log(); })), nullptr);
    g_signal_connect_swapped(g_stop_button, "clicked", G_CALLBACK((+[](gpointer){ stop_backend(); })), nullptr);
    g_signal_connect_swapped(g_apply_walk_button, "clicked", G_CALLBACK((+[](gpointer){ apply_walk_settings(); })), nullptr);
    g_signal_connect_swapped(g_start_normal_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) send_line("walk-normal"); })), nullptr);
    g_signal_connect_swapped(g_start_mail_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) send_line("walk-mail"); })), nullptr);
    g_signal_connect_swapped(g_walk_status_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) { send_line("walk-settings"); send_line("P"); } })), nullptr);
    g_signal_connect_swapped(g_stop_walk_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) send_line("O"); })), nullptr);
    g_signal_connect_swapped(g_route_status_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) send_line("C"); })), nullptr);
    g_signal_connect_swapped(g_node_list_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) send_line("I"); })), nullptr);
    g_signal_connect_swapped(g_daemon_status_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) send_line("D"); })), nullptr);
    g_signal_connect_swapped(g_copy_main_header_button, "clicked", G_CALLBACK((+[](gpointer){ copy_text(text_view_text(g_main_header_view)); })), nullptr);
    g_signal_connect_swapped(g_copy_mail_header_button, "clicked", G_CALLBACK((+[](gpointer){ copy_text(text_view_text(g_mail_header_view)); })), nullptr);
    g_signal_connect_swapped(g_refresh_headers_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) send_line("headers"); })), nullptr);
    g_signal_connect_swapped(g_app_register_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) { const auto id=entry_text(g_app_id_entry), name=entry_text(g_app_name_entry); if(!id.empty()&&!name.empty()) send_lines({"app-add",id,name}); } })), nullptr);
    g_signal_connect_swapped(g_app_list_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) send_line("app-list"); })), nullptr);
    g_signal_connect_swapped(g_app_rotate_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) { const auto id=entry_text(g_app_id_entry); if(!id.empty()) send_lines({"app-rotate",id}); } })), nullptr);
    g_signal_connect_swapped(g_app_pending_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) send_line("app-pending"); })), nullptr);
    g_signal_connect_swapped(g_approve_button, "clicked", G_CALLBACK((+[](gpointer){ act_on_checked_requests(true); })), nullptr);
    g_signal_connect_swapped(g_reject_button, "clicked", G_CALLBACK((+[](gpointer){ act_on_checked_requests(false); })), nullptr);
    g_signal_connect_swapped(g_set_default_visible_name_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) { const auto name=entry_text(g_visible_name_entry); if(!name.empty()) send_line("app-name default "+name); } })), nullptr);
    g_signal_connect_swapped(g_set_app_alias_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) { const auto id=entry_text(g_app_id_entry), name=entry_text(g_visible_name_entry); if(!id.empty()&&!name.empty()) send_line("app-name set "+id+" "+name); } })), nullptr);
    g_signal_connect_swapped(g_clear_app_alias_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) { const auto id=entry_text(g_app_id_entry); if(!id.empty()) send_line("app-name clear "+id); } })), nullptr);
    g_signal_connect_swapped(g_list_visible_names_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) send_line("app-name"); })), nullptr);
    g_signal_connect_swapped(g_profile_create_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) { const auto name=entry_text(g_profile_name_entry); if(!name.empty()) send_line("profile-create "+name); } })), nullptr);
    g_signal_connect_swapped(g_profile_list_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) send_line("profile-list"); })), nullptr);
    g_signal_connect_swapped(g_profile_use_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) { const auto id=entry_text(g_profile_id_entry); if(!id.empty()) send_line("profile-use "+id); } })), nullptr);
    g_signal_connect_swapped(g_profile_retire_button, "clicked", G_CALLBACK((+[](gpointer){ if (require_ready()) { const auto id=entry_text(g_profile_id_entry); if(!id.empty()) send_line("profile-retire "+id); } })), nullptr);
    g_signal_connect_swapped(g_password_entry, "activate", G_CALLBACK((+[](gpointer){ authenticate(false); })), nullptr);

    apply_language();
    set_status(T("Waiting for login"));
}

} // namespace

int main(int argc, char** argv) {
    gtk_init(&argc, &argv);
    load_language();
    build_ui();
    gtk_widget_show_all(g_window);
    g_timeout_add_seconds(15, +[](gpointer) -> gboolean {
        if (g_ready) { send_line("summary"); send_line("app-pending"); }
        return G_SOURCE_CONTINUE;
    }, nullptr);
    gtk_main();
    if (g_reader_thread.joinable()) g_reader_thread.join();
    return 0;
}

#include <windows.h>
#include <commctrl.h>
#include <dwmapi.h>
#include <shellapi.h>
#include <uxtheme.h>

#include <algorithm>
#include <cstring>
#include <cwctype>
#include <atomic>
#include <filesystem>
#include <initializer_list>
#include <limits>
#include <mutex>
#include <sstream>
#include <string>
#include <thread>
#include <vector>

#include "ui_localization.h"

#pragma comment(lib, "Comctl32.lib")
#pragma comment(lib, "Dwmapi.lib")
#pragma comment(lib, "UxTheme.lib")

namespace {

constexpr wchar_t kWindowClass[] = L"VeilKnitDaemonGuiWindow";
constexpr wchar_t kPageClass[] = L"VeilKnitDaemonGuiPage";
constexpr wchar_t kWindowTitle[] = L"VeilKnit Daemon";

constexpr UINT WM_APP_LOG_LINE = WM_APP + 1;
constexpr UINT WM_APP_BACKEND_EXIT = WM_APP + 2;
constexpr UINT WM_APP_TRAY = WM_APP + 3;

constexpr int ID_TAB = 100;
constexpr int ID_USERNAME = 110;
constexpr int ID_PASSWORD = 111;
constexpr int ID_LOGIN = 112;
constexpr int ID_SIGNUP = 113;
constexpr int ID_MAIN_KEY = 120;
constexpr int ID_COPY_KEY = 121;
constexpr int ID_MINIMIZE_TO_TRAY = 122;
constexpr int ID_SAVE_LOG = 123;
constexpr int ID_SHUTDOWN = 124;
constexpr int ID_HELP = 125;
constexpr int ID_REMEMBER_CLOSE = 126;
constexpr int ID_LANGUAGE = 127;
constexpr int ID_HANDSHAKE_KEY = 130;
constexpr int ID_HANDSHAKE_START = 131;
constexpr int ID_HANDSHAKE_STATUS = 132;

constexpr int ID_WALK_NORMAL_MIN_HOPS = 210;
constexpr int ID_WALK_NORMAL_MAX_HOPS = 211;
constexpr int ID_WALK_NORMAL_MIN_SECS = 212;
constexpr int ID_WALK_NORMAL_TARGET_SECS = 213;
constexpr int ID_WALK_NORMAL_MAX_SECS = 214;
constexpr int ID_WALK_MAIL_MIN_HOPS = 215;
constexpr int ID_WALK_MAIL_MAX_HOPS = 216;
constexpr int ID_WALK_MAIL_MIN_SECS = 217;
constexpr int ID_WALK_MAIL_TARGET_SECS = 218;
constexpr int ID_WALK_MAIL_MAX_SECS = 219;
constexpr int ID_WALK_MAIL_MODE = 220;
constexpr int ID_WALK_APPLY = 221;
constexpr int ID_WALK_NORMAL_START = 222;
constexpr int ID_WALK_MAIL_START = 223;
constexpr int ID_WALK_STATUS = 224;
constexpr int ID_WALK_STOP = 225;
constexpr int ID_ROUTE_STATUS = 226;
constexpr int ID_NODE_LIST = 227;
constexpr int ID_DAEMON_STATUS = 228;

constexpr int ID_HEADERS_REFRESH = 240;
constexpr int ID_HEADERS_COPY_MAIN = 241;
constexpr int ID_HEADERS_COPY_MAILBOX = 242;

constexpr int ID_DHT_NAME = 150;
constexpr int ID_DHT_GROUPS = 151;
constexpr int ID_DHT_CREATE = 152;
constexpr int ID_DHT_INDEX = 153;
constexpr int ID_DHT_SUBKEY = 154;
constexpr int ID_DHT_DATA = 155;
constexpr int ID_DHT_INSPECT = 156;
constexpr int ID_DHT_WRITE = 157;
constexpr int ID_DHT_READ = 158;
constexpr int ID_DHT_READ_ALL = 159;
constexpr int ID_DHT_SAVE = 160;
constexpr int ID_DHT_EXTERNAL_KEY = 161;
constexpr int ID_DHT_LOCATIONS = 162;
constexpr int ID_DHT_EXTERNAL_SELECTED = 163;
constexpr int ID_DHT_EXTERNAL_ALL = 164;

constexpr int ID_MAIL_RECIPIENT = 170;
constexpr int ID_MAIL_APP = 171;
constexpr int ID_MAIL_PAYLOAD = 172;
constexpr int ID_MAIL_SEND = 173;
constexpr int ID_MAIL_STATUS = 174;
constexpr int ID_MAIL_LIST = 175;
constexpr int ID_MAIL_RETRIEVE = 176;
constexpr int ID_MAIL_STATS = 177;
constexpr int ID_MAIL_FLUSH = 178;
constexpr int ID_MAIL_REPAIR = 179;

constexpr int ID_APP_ID = 180;
constexpr int ID_APP_NAME = 181;
constexpr int ID_APP_ADD = 182;
constexpr int ID_APP_LIST = 183;
constexpr int ID_APP_ROTATE = 184;
constexpr int ID_APP_PENDING = 185;
constexpr int ID_APP_REQUEST = 186;
constexpr int ID_APP_APPROVE = 187;
constexpr int ID_APP_REASON = 188;
constexpr int ID_APP_REJECT = 189;

constexpr int ID_TRAY_OPEN = 200;
constexpr int ID_TRAY_PROPER = 201;
constexpr int ID_TRAY_CRAZY = 202;
constexpr int ID_CLOSE_PROPER = 301;
constexpr int ID_CLOSE_CRAZY = 302;
constexpr int ID_CLOSE_TRAY = 303;

constexpr COLORREF kColorWindow = RGB(19, 19, 21);
constexpr COLORREF kColorPanel = RGB(28, 28, 31);
constexpr COLORREF kColorEdit = RGB(22, 22, 24);
constexpr COLORREF kColorText = RGB(235, 235, 238);
constexpr COLORREF kColorMuted = RGB(165, 165, 172);
constexpr COLORREF kColorRed = RGB(239, 35, 60);
constexpr COLORREF kColorRedDark = RGB(145, 18, 35);
constexpr COLORREF kColorBorder = RGB(65, 65, 71);
constexpr COLORREF kColorWarning = RGB(255, 168, 64);
constexpr COLORREF kColorSuccess = RGB(85, 194, 113);

struct Page {
    HWND container{};
    HWND log{};
};

HINSTANCE gInstance{};
HWND gMainWindow{};
HWND gHeaderTitle{};
HWND gStatusLabel{};
HWND gTab{};
std::vector<Page> gPages;

HWND gUsernameLabel{};
HWND gUsernameEdit{};
HWND gPasswordLabel{};
HWND gPasswordEdit{};
HWND gLoginButton{};
HWND gSignupButton{};
HWND gMainKeyLabel{};
HWND gMainKeyEdit{};
HWND gCopyKeyButton{};
HWND gMinimizeCheckbox{};
HWND gRememberCloseCheckbox{};
HWND gSaveLogButton{};
HWND gShutdownButton{};
HWND gHelpButton{};
HWND gLanguageLabel{};
HWND gLanguageCombo{};
UiLanguage gLanguage = UiLanguage::English;
WNDPROC gOldUsernameProc{};
WNDPROC gOldPasswordProc{};
COLORREF gStatusColor = kColorRed;

HWND gHandshakeLabel{};
HWND gHandshakeEdit{};
HWND gHandshakeStartButton{};
HWND gHandshakeStatusButton{};
HWND gHandshakeResultLabel{};

HWND gWalkNormalLabel{};
HWND gWalkNormalMinHopsLabel{};
HWND gWalkNormalMinHopsEdit{};
HWND gWalkNormalMaxHopsLabel{};
HWND gWalkNormalMaxHopsEdit{};
HWND gWalkNormalMinSecsLabel{};
HWND gWalkNormalMinSecsEdit{};
HWND gWalkNormalTargetSecsLabel{};
HWND gWalkNormalTargetSecsEdit{};
HWND gWalkNormalMaxSecsLabel{};
HWND gWalkNormalMaxSecsEdit{};
HWND gWalkMailLabel{};
HWND gWalkMailMinHopsLabel{};
HWND gWalkMailMinHopsEdit{};
HWND gWalkMailMaxHopsLabel{};
HWND gWalkMailMaxHopsEdit{};
HWND gWalkMailMinSecsLabel{};
HWND gWalkMailMinSecsEdit{};
HWND gWalkMailTargetSecsLabel{};
HWND gWalkMailTargetSecsEdit{};
HWND gWalkMailMaxSecsLabel{};
HWND gWalkMailMaxSecsEdit{};
HWND gWalkMailModeCheckbox{};
HWND gWalkApplyButton{};
HWND gWalkNormalStartButton{};
HWND gWalkMailStartButton{};
HWND gWalkStatusButton{};
HWND gWalkStopButton{};
HWND gRouteStatusButton{};
HWND gNodeListButton{};
HWND gDaemonStatusButton{};

HWND gMainHeaderLabel{};
HWND gMainHeaderEdit{};
HWND gCopyMainHeaderButton{};
HWND gMailboxHeaderLabel{};
HWND gMailboxHeaderEdit{};
HWND gCopyMailboxHeaderButton{};
HWND gRefreshHeadersButton{};

HWND gDhtNameLabel{};
HWND gDhtNameEdit{};
HWND gDhtGroupsLabel{};
HWND gDhtGroupsEdit{};
HWND gDhtCreateButton{};
HWND gDhtIndexLabel{};
HWND gDhtIndexEdit{};
HWND gDhtSubkeyLabel{};
HWND gDhtSubkeyEdit{};
HWND gDhtDataLabel{};
HWND gDhtDataEdit{};
HWND gDhtInspectButton{};
HWND gDhtWriteButton{};
HWND gDhtReadButton{};
HWND gDhtReadAllButton{};
HWND gDhtSaveButton{};
HWND gDhtExternalKeyLabel{};
HWND gDhtExternalKeyEdit{};
HWND gDhtLocationsLabel{};
HWND gDhtLocationsEdit{};
HWND gDhtExternalSelectedButton{};
HWND gDhtExternalAllButton{};

HWND gMailRecipientLabel{};
HWND gMailRecipientEdit{};
HWND gMailAppLabel{};
HWND gMailAppEdit{};
HWND gMailPayloadLabel{};
HWND gMailPayloadEdit{};
HWND gMailSendButton{};
HWND gMailStatusButton{};
HWND gMailListButton{};
HWND gMailRetrieveButton{};
HWND gMailStatsButton{};
HWND gMailFlushButton{};
HWND gMailRepairButton{};

HWND gAppIdLabel{};
HWND gAppIdEdit{};
HWND gAppNameLabel{};
HWND gAppNameEdit{};
HWND gAppAddButton{};
HWND gAppListButton{};
HWND gAppRotateButton{};
HWND gAppPendingButton{};
HWND gAppRequestLabel{};
HWND gAppRequestEdit{};
HWND gAppApproveButton{};
HWND gAppReasonLabel{};
HWND gAppReasonEdit{};
HWND gAppRejectButton{};

HFONT gUiFont{};
HFONT gTitleFont{};
HBRUSH gWindowBrush{};
HBRUSH gPanelBrush{};
HBRUSH gEditBrush{};

HANDLE gProcess{};
HANDLE gProcessStdin{};
HANDLE gProcessStdout{};
std::thread gReaderThread;
std::mutex gProcessMutex;
std::atomic<bool> gProcessRunning{false};
std::atomic<bool> gAuthenticated{false};
std::atomic<bool> gReady{false};
std::atomic<bool> gClosingProperly{false};

NOTIFYICONDATAW gTrayData{};
bool gTrayAdded = false;
int gSavedCloseAction = 0;
std::wstring gMainDhtKey;

bool SendBackendLine(const std::wstring& line);

const wchar_t* T(const wchar_t* english) {
    return UiText(gLanguage, english);
}

std::filesystem::path UiSettingsPath() {
    std::vector<wchar_t> buffer(32768);
    const DWORD length = GetModuleFileNameW(nullptr, buffer.data(), static_cast<DWORD>(buffer.size()));
    std::filesystem::path path(std::wstring(buffer.data(), length));
    return path.parent_path() / L"veilknit_ui.ini";
}

UiLanguage LoadUiLanguage() {
    const std::wstring path = UiSettingsPath().wstring();
    const int value = GetPrivateProfileIntW(L"interface", L"language", 0, path.c_str());
    return value >= 0 && value <= 4 ? static_cast<UiLanguage>(value) : UiLanguage::English;
}

void SaveUiLanguage() {
    const std::wstring path = UiSettingsPath().wstring();
    const std::wstring value = std::to_wstring(static_cast<int>(gLanguage));
    WritePrivateProfileStringW(L"interface", L"language", value.c_str(), path.c_str());
}

void SetUiText(HWND control, const wchar_t* english) {
    if (control) SetWindowTextW(control, T(english));
}

void ApplyLanguage() {
    if (!gMainWindow) return;
    SetWindowTextW(gMainWindow, T(L"VeilKnit Daemon"));
    SetUiText(gHeaderTitle, L"VeilKnit Daemon");
    SetUiText(gUsernameLabel, L"Username");
    SetUiText(gPasswordLabel, L"Password");
    SetUiText(gLanguageLabel, L"Language");
    SetUiText(gLoginButton, L"Login");
    SetUiText(gSignupButton, L"Sign up");
    SetUiText(gMainKeyLabel, L"Main key");
    SetUiText(gCopyKeyButton, L"Copy key");
    SetUiText(gMinimizeCheckbox, L"Minimize to notification area");
    SetUiText(gRememberCloseCheckbox, L"Always use saved X-button action");
    SetUiText(gSaveLogButton, L"Save log");
    SetUiText(gShutdownButton, L"Close properly");
    SetUiText(gHelpButton, L"Help");
    SetUiText(gHandshakeLabel, L"Peer VLD0 key");
    SetUiText(gHandshakeStartButton, L"Establish handshake");
    SetUiText(gHandshakeStatusButton, L"Check status");
    if (!gAuthenticated) SetUiText(gHandshakeResultLabel, L"No handshake action requested yet.");
    SetUiText(gWalkNormalLabel, L"Normal mode");
    SetUiText(gWalkNormalMinHopsLabel, L"Min hops");
    SetUiText(gWalkNormalMaxHopsLabel, L"Max hops");
    SetUiText(gWalkNormalMinSecsLabel, L"Min sec");
    SetUiText(gWalkNormalTargetSecsLabel, L"Target sec");
    SetUiText(gWalkNormalMaxSecsLabel, L"Max sec");
    SetUiText(gWalkMailLabel, L"Mail mode");
    SetUiText(gWalkMailMinHopsLabel, L"Min hops");
    SetUiText(gWalkMailMaxHopsLabel, L"Max hops");
    SetUiText(gWalkMailMinSecsLabel, L"Min sec");
    SetUiText(gWalkMailTargetSecsLabel, L"Target sec");
    SetUiText(gWalkMailMaxSecsLabel, L"Max sec");
    SetUiText(gWalkMailModeCheckbox, L"Use mail mode for automatic walks");
    SetUiText(gWalkApplyButton, L"Apply settings");
    SetUiText(gWalkNormalStartButton, L"Start normal walk");
    SetUiText(gWalkMailStartButton, L"Start mail walk");
    SetUiText(gWalkStatusButton, L"Walk status");
    SetUiText(gWalkStopButton, L"Stop walk");
    SetUiText(gRouteStatusButton, L"Route status");
    SetUiText(gNodeListButton, L"Node list");
    SetUiText(gDaemonStatusButton, L"Daemon status");
    SetUiText(gMainHeaderLabel, L"Subkey 0 - main/presence header");
    SetUiText(gCopyMainHeaderButton, L"Copy main header");
    SetUiText(gMailboxHeaderLabel, L"Subkey 2 - mailbox advertisement header");
    SetUiText(gCopyMailboxHeaderButton, L"Copy mailbox header");
    SetUiText(gRefreshHeadersButton, L"Refresh headers");
    SetUiText(gDhtNameLabel, L"Name");
    SetUiText(gDhtGroupsLabel, L"Owner groups");
    SetUiText(gDhtCreateButton, L"Create DHT");
    SetUiText(gDhtIndexLabel, L"Index");
    SetUiText(gDhtSubkeyLabel, L"Subkey");
    SetUiText(gDhtDataLabel, L"Value");
    SetUiText(gDhtInspectButton, L"Inspect");
    SetUiText(gDhtWriteButton, L"Write");
    SetUiText(gDhtReadButton, L"Read");
    SetUiText(gDhtReadAllButton, L"Read all");
    SetUiText(gDhtSaveButton, L"Save DHTs");
    SetUiText(gDhtExternalKeyLabel, L"External VLD0 key");
    SetUiText(gDhtLocationsLabel, L"Subkeys");
    SetUiText(gDhtExternalSelectedButton, L"Read selected");
    SetUiText(gDhtExternalAllButton, L"Read all external");
    SetUiText(gMailRecipientLabel, L"Recipient VLD0 key");
    SetUiText(gMailAppLabel, L"Application");
    SetUiText(gMailPayloadLabel, L"Payload");
    SetUiText(gMailSendButton, L"Send mail");
    SetUiText(gMailStatusButton, L"Status");
    SetUiText(gMailListButton, L"List inbox");
    SetUiText(gMailRetrieveButton, L"Retrieve");
    SetUiText(gMailStatsButton, L"Statistics");
    SetUiText(gMailFlushButton, L"Flush");
    SetUiText(gMailRepairButton, L"Repair");
    SetUiText(gAppIdLabel, L"Application id");
    SetUiText(gAppNameLabel, L"Display name");
    SetUiText(gAppAddButton, L"Add app");
    SetUiText(gAppListButton, L"List apps");
    SetUiText(gAppRotateButton, L"Rotate key");
    SetUiText(gAppPendingButton, L"Pending requests");
    SetUiText(gAppRequestLabel, L"Request id");
    SetUiText(gAppApproveButton, L"Approve");
    SetUiText(gAppReasonLabel, L"Reject reason");
    SetUiText(gAppRejectButton, L"Reject");

    const wchar_t* tabs[] = {L"Overview", L"Handshake", L"Network", L"Headers", L"DHT", L"Mailbox", L"Applications", L"All logs"};
    for (int index = 0; index < 8; ++index) {
        TCITEMW item{};
        item.mask = TCIF_TEXT;
        item.pszText = const_cast<wchar_t*>(T(tabs[index]));
        TabCtrl_SetItem(gTab, index, &item);
    }
    InvalidateRect(gMainWindow, nullptr, TRUE);
}

std::wstring ToLower(std::wstring value) {
    std::transform(value.begin(), value.end(), value.begin(), [](wchar_t character) {
        return static_cast<wchar_t>(towlower(character));
    });
    return value;
}

bool ContainsAny(const std::wstring& haystack, std::initializer_list<const wchar_t*> needles) {
    for (const wchar_t* needle : needles) {
        if (haystack.find(needle) != std::wstring::npos) {
            return true;
        }
    }
    return false;
}

std::wstring Utf8ToWide(const std::string& value) {
    if (value.empty()) {
        return {};
    }
    int required = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                                       static_cast<int>(value.size()), nullptr, 0);
    UINT codePage = CP_UTF8;
    DWORD flags = MB_ERR_INVALID_CHARS;
    if (required == 0) {
        codePage = CP_ACP;
        flags = 0;
        required = MultiByteToWideChar(codePage, flags, value.data(),
                                       static_cast<int>(value.size()), nullptr, 0);
    }
    std::wstring result(static_cast<size_t>(required), L'\0');
    if (required > 0) {
        MultiByteToWideChar(codePage, flags, value.data(), static_cast<int>(value.size()),
                            result.data(), required);
    }
    return result;
}

std::string WideToUtf8(const std::wstring& value) {
    if (value.empty()) {
        return {};
    }
    int required = WideCharToMultiByte(CP_UTF8, 0, value.data(), static_cast<int>(value.size()),
                                       nullptr, 0, nullptr, nullptr);
    std::string result(static_cast<size_t>(required), '\0');
    WideCharToMultiByte(CP_UTF8, 0, value.data(), static_cast<int>(value.size()),
                        result.data(), required, nullptr, nullptr);
    return result;
}

std::wstring WindowText(HWND window) {
    int length = GetWindowTextLengthW(window);
    std::wstring value(static_cast<size_t>(length) + 1, L'\0');
    GetWindowTextW(window, value.data(), length + 1);
    value.resize(static_cast<size_t>(length));
    return value;
}

std::wstring DecodeEscapedGuiValue(const std::wstring& value) {
    std::wstring decoded;
    decoded.reserve(value.size());
    bool escaped = false;
    for (wchar_t character : value) {
        if (!escaped) {
            if (character == L'\\') {
                escaped = true;
            } else {
                decoded.push_back(character);
            }
            continue;
        }

        switch (character) {
        case L'n': decoded += L"\r\n"; break;
        case L'r': break;
        case L't': decoded.push_back(L'\t'); break;
        case L'\\': decoded.push_back(L'\\'); break;
        default:
            decoded.push_back(L'\\');
            decoded.push_back(character);
            break;
        }
        escaped = false;
    }
    if (escaped) {
        decoded.push_back(L'\\');
    }
    return decoded;
}

std::vector<std::wstring> SplitCommaSeparated(const std::wstring& value) {
    std::vector<std::wstring> parts;
    std::wstringstream stream(value);
    std::wstring part;
    while (std::getline(stream, part, L',')) {
        parts.push_back(part);
    }
    return parts;
}

void ApplyWalkSettingsMarker(const std::wstring& payload) {
    const std::vector<std::wstring> values = SplitCommaSeparated(payload);
    if (values.size() != 11) {
        return;
    }
    HWND edits[] = {
        gWalkNormalMinHopsEdit, gWalkNormalMaxHopsEdit,
        gWalkNormalMinSecsEdit, gWalkNormalTargetSecsEdit, gWalkNormalMaxSecsEdit,
        gWalkMailMinHopsEdit, gWalkMailMaxHopsEdit,
        gWalkMailMinSecsEdit, gWalkMailTargetSecsEdit, gWalkMailMaxSecsEdit,
    };
    for (size_t index = 0; index < ARRAYSIZE(edits); ++index) {
        if (edits[index]) {
            SetWindowTextW(edits[index], values[index].c_str());
        }
    }
    if (gWalkMailModeCheckbox) {
        const bool enabled = values[10] == L"1" || ToLower(values[10]) == L"true";
        SendMessageW(gWalkMailModeCheckbox, BM_SETCHECK,
                     enabled ? BST_CHECKED : BST_UNCHECKED, 0);
    }
}

void SetStatus(const std::wstring& text) {
    if (gStatusLabel) {
        SetWindowTextW(gStatusLabel, text.c_str());
        InvalidateRect(gStatusLabel, nullptr, TRUE);
    }
}

void SetConnectionStatus(const std::wstring& text, COLORREF color) {
    gStatusColor = color;
    SetStatus(text);
}

void ShowHelp() {
    MessageBoxW(
        gMainWindow,
        T(L"Getting started:\n\n"
          L"1. Enter your username and password, then press Enter to log in.\n"
          L"2. Applications request permission through the Applications tab.\n"
          L"3. Look for a line such as:\n"
          L"   [api] Application authorization requested: #1 veilknit.rooms\n\n"
          L"The important value is the request number (#1 in this example). "
          L"Enter 1 in Request id and choose Approve.\n\n"
          L"The Network tab contains separate adaptive settings and manual start buttons "
          L"for normal and mail walks. The Headers tab reads and displays your published "
          L"subkey 0 presence header and subkey 2 mailbox advertisement.\n\n"
          L"Green status means ready, orange means starting or reconnecting, and red means "
          L"stopped or failed."),
        T(L"VeilKnit Daemon Help"),
        MB_OK | MB_ICONINFORMATION);
}

LRESULT CALLBACK CredentialEditProc(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam) {
    if (message == WM_KEYDOWN) {
        if (wparam == VK_TAB) {
            SetFocus(hwnd == gUsernameEdit ? gPasswordEdit : gUsernameEdit);
            return 0;
        }
        if (hwnd == gPasswordEdit && wparam == VK_RETURN) {
            SendMessageW(gMainWindow, WM_COMMAND, MAKEWPARAM(ID_LOGIN, BN_CLICKED),
                         reinterpret_cast<LPARAM>(gLoginButton));
            return 0;
        }
    }
    WNDPROC original = hwnd == gUsernameEdit ? gOldUsernameProc : gOldPasswordProc;
    return CallWindowProcW(original, hwnd, message, wparam, lparam);
}

void ApplyFont(HWND control, HFONT font = nullptr) {
    SendMessageW(control, WM_SETFONT, reinterpret_cast<WPARAM>(font ? font : gUiFont), TRUE);
}

HWND CreateLabel(HWND parent, const wchar_t* text, int id = 0) {
    HWND control = CreateWindowExW(0, L"STATIC", text, WS_CHILD | WS_VISIBLE | SS_LEFT,
                                   0, 0, 10, 10, parent,
                                   reinterpret_cast<HMENU>(static_cast<INT_PTR>(id)), gInstance, nullptr);
    ApplyFont(control);
    return control;
}

HWND CreateEdit(HWND parent, int id, DWORD extraStyle = 0) {
    HWND control = CreateWindowExW(WS_EX_CLIENTEDGE, L"EDIT", L"",
                                   WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL | extraStyle,
                                   0, 0, 10, 10, parent,
                                   reinterpret_cast<HMENU>(static_cast<INT_PTR>(id)), gInstance, nullptr);
    ApplyFont(control);
    SetWindowTheme(control, L"DarkMode_Explorer", nullptr);
    return control;
}

HWND CreateButton(HWND parent, const wchar_t* text, int id) {
    HWND control = CreateWindowExW(0, L"BUTTON", text,
                                   WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW,
                                   0, 0, 10, 10, parent,
                                   reinterpret_cast<HMENU>(static_cast<INT_PTR>(id)), gInstance, nullptr);
    ApplyFont(control);
    return control;
}

HWND CreateLog(HWND parent) {
    HWND control = CreateWindowExW(WS_EX_CLIENTEDGE, L"EDIT", L"",
                                   WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_HSCROLL |
                                       ES_MULTILINE | ES_AUTOVSCROLL | ES_AUTOHSCROLL | ES_READONLY,
                                   0, 0, 10, 10, parent, nullptr, gInstance, nullptr);
    ApplyFont(control);
    SendMessageW(control, EM_SETLIMITTEXT, 4 * 1024 * 1024, 0);
    SetWindowTheme(control, L"DarkMode_Explorer", nullptr);
    return control;
}

void AppendToLog(HWND log, const std::wstring& line) {
    if (!log) {
        return;
    }

    constexpr int kTrimThreshold = 2 * 1024 * 1024;
    constexpr int kTrimAmount = 256 * 1024;
    int currentLength = GetWindowTextLengthW(log);
    if (currentLength > kTrimThreshold) {
        SendMessageW(log, EM_SETREADONLY, FALSE, 0);
        SendMessageW(log, EM_SETSEL, 0, kTrimAmount);
        SendMessageW(log, EM_REPLACESEL, FALSE, reinterpret_cast<LPARAM>(L""));
        SendMessageW(log, EM_SETREADONLY, TRUE, 0);
    }

    std::wstring output = line;
    output += L"\r\n";
    SendMessageW(log, EM_SETREADONLY, FALSE, 0);
    SendMessageW(log, EM_SETSEL, static_cast<WPARAM>(-1), static_cast<LPARAM>(-1));
    SendMessageW(log, EM_REPLACESEL, FALSE, reinterpret_cast<LPARAM>(output.c_str()));
    SendMessageW(log, EM_SETREADONLY, TRUE, 0);
    SendMessageW(log, EM_SCROLLCARET, 0, 0);
}

void EnableCredentialControls(bool enabled) {
    EnableWindow(gUsernameEdit, enabled);
    EnableWindow(gPasswordEdit, enabled);
    EnableWindow(gLoginButton, enabled);
    EnableWindow(gSignupButton, enabled);
}

void ProcessLogLine(const std::wstring& line) {
    if (gPages.size() < 8) {
        return;
    }

    const std::wstring lower = ToLower(line);
    AppendToLog(gPages[7].log, line); // All logs

    if (ContainsAny(lower, {L"welcome", L"starting", L"ready", L"warning", L"failed",
                            L"shutdown", L"main dht", L"main-dht", L"network core",
                            L"mailbox controller", L"local application api"})) {
        AppendToLog(gPages[0].log, line);
    }
    if (lower.find(L"handshake") != std::wstring::npos) {
        AppendToLog(gPages[1].log, line);
        SetWindowTextW(gHandshakeResultLabel, line.c_str());
    }
    if (ContainsAny(lower, {L"[node]", L"network", L"walk", L"route", L"attachment", L"veilid",
                            L"presence", L"needs refresh", L"stale online claim"})) {
        AppendToLog(gPages[2].log, line);
    }
    if (ContainsAny(lower, {L"header", L"presence subkey", L"mailbox advertisement"})) {
        AppendToLog(gPages[3].log, line);
    }
    if (ContainsAny(lower, {L"dht", L"record key", L"subkey"})) {
        AppendToLog(gPages[4].log, line);
    }
    if (ContainsAny(lower, {L"mail", L"mailbox", L"outbox", L"inbox"})) {
        AppendToLog(gPages[5].log, line);
    }
    if (ContainsAny(lower, {L"[api]", L"application", L"app-", L"identity", L"reputation"})) {
        AppendToLog(gPages[6].log, line);
    }

    const std::wstring keyMarker = L"MAIN_DHT_KEY=";
    size_t markerPosition = line.find(keyMarker);
    if (markerPosition != std::wstring::npos) {
        gMainDhtKey = line.substr(markerPosition + keyMarker.size());
        SetWindowTextW(gMainKeyEdit, gMainDhtKey.c_str());
    }

    const std::wstring walkMarker = L"WALK_SETTINGS=";
    markerPosition = line.find(walkMarker);
    if (markerPosition != std::wstring::npos) {
        ApplyWalkSettingsMarker(line.substr(markerPosition + walkMarker.size()));
    }

    const std::wstring mainHeaderMarker = L"MAIN_HEADER=";
    markerPosition = line.find(mainHeaderMarker);
    if (markerPosition != std::wstring::npos && gMainHeaderEdit) {
        const std::wstring decoded = DecodeEscapedGuiValue(
            line.substr(markerPosition + mainHeaderMarker.size()));
        SetWindowTextW(gMainHeaderEdit, decoded.c_str());
    }

    const std::wstring mailboxHeaderMarker = L"MAILBOX_HEADER=";
    markerPosition = line.find(mailboxHeaderMarker);
    if (markerPosition != std::wstring::npos && gMailboxHeaderEdit) {
        const std::wstring decoded = DecodeEscapedGuiValue(
            line.substr(markerPosition + mailboxHeaderMarker.size()));
        SetWindowTextW(gMailboxHeaderEdit, decoded.c_str());
    }

    if (lower.find(L"welcome,") != std::wstring::npos) {
        gAuthenticated = true;
        SetConnectionStatus(T(L"Authenticated; starting network services..."), kColorWarning);
        SetWindowTextW(gPasswordEdit, L"");
    }

    if (line.find(L"[gui] READY") != std::wstring::npos) {
        gReady = true;
        EnableCredentialControls(false);
        SetConnectionStatus(T(L"Running"), kColorSuccess);
        SendBackendLine(L"walk-settings");
        SendBackendLine(L"headers");
    }

    if (ContainsAny(lower, {L"no account with that username", L"wrong password",
                            L"username is already taken", L"usernames may only contain"})) {
        gAuthenticated = false;
        EnableCredentialControls(true);
        SetConnectionStatus(T(L"Authentication failed; correct the details and try again."), kColorRed);
        SetFocus(gPasswordEdit);
    }
}

std::filesystem::path ExecutableDirectory() {
    std::vector<wchar_t> buffer(32768);
    DWORD length = GetModuleFileNameW(nullptr, buffer.data(), static_cast<DWORD>(buffer.size()));
    return std::filesystem::path(std::wstring(buffer.data(), length)).parent_path();
}

std::filesystem::path FindBackendExecutable() {
    const auto directory = ExecutableDirectory();
    const std::vector<std::filesystem::path> candidates = {
        directory / L"veilid_test_node.exe",
        directory / L"backend" / L"veilid_test_node.exe",
        directory.parent_path().parent_path().parent_path() / L"target" / L"release" / L"veilid_test_node.exe",
        directory.parent_path().parent_path().parent_path() / L"target" / L"debug" / L"veilid_test_node.exe",
    };
    for (const auto& candidate : candidates) {
        std::error_code error;
        if (std::filesystem::exists(candidate, error)) {
            return candidate;
        }
    }
    return {};
}

bool WriteBackendBytes(const std::string& bytes) {
    std::lock_guard<std::mutex> lock(gProcessMutex);
    if (!gProcessStdin || !gProcessRunning) {
        return false;
    }
    DWORD written = 0;
    return WriteFile(gProcessStdin, bytes.data(), static_cast<DWORD>(bytes.size()), &written, nullptr) &&
           written == static_cast<DWORD>(bytes.size());
}

bool SendBackendLine(const std::wstring& line) {
    std::string bytes = WideToUtf8(line);
    bytes.push_back('\n');
    return WriteBackendBytes(bytes);
}

bool SendBackendLines(const std::vector<std::wstring>& lines) {
    std::wstring payload;
    for (const std::wstring& line : lines) {
        payload += line;
        payload += L"\n";
    }
    return WriteBackendBytes(WideToUtf8(payload));
}

bool RequireReady() {
    if (gReady) {
        return true;
    }
    MessageBoxW(gMainWindow, T(L"The daemon is not ready yet."), T(L"VeilKnit Daemon"),
                MB_OK | MB_ICONINFORMATION);
    return false;
}

bool IsSingleLine(const std::wstring& value) {
    return value.find_first_of(L"\r\n") == std::wstring::npos;
}

bool IsUnsignedInteger(
    const std::wstring& value,
    unsigned long long maximum = std::numeric_limits<unsigned long long>::max()) {
    if (value.empty() || !std::all_of(value.begin(), value.end(), [](wchar_t character) {
            return character >= L'0' && character <= L'9';
        })) {
        return false;
    }
    try {
        size_t consumed = 0;
        const unsigned long long parsed = std::stoull(value, &consumed, 10);
        return consumed == value.size() && parsed <= maximum;
    } catch (...) {
        return false;
    }
}

bool IsBase64UrlSegment(const std::wstring& value) {
    return value.size() == 43 && std::all_of(value.begin(), value.end(), [](wchar_t character) {
        return (character >= L'a' && character <= L'z') ||
               (character >= L'A' && character <= L'Z') ||
               (character >= L'0' && character <= L'9') ||
               character == L'_' || character == L'-';
    });
}

bool LooksLikeRecordKey(const std::wstring& value) {
    if (value.rfind(L"VLD0:", 0) != 0) {
        return false;
    }
    const size_t secondColon = value.find(L':', 5);
    if (secondColon == std::wstring::npos || value.find(L':', secondColon + 1) != std::wstring::npos) {
        return false;
    }
    return IsBase64UrlSegment(value.substr(5, secondColon - 5)) &&
           IsBase64UrlSegment(value.substr(secondColon + 1));
}

bool LooksLikeSubkeySelection(const std::wstring& input) {
    std::wstringstream stream(input);
    std::wstring part;
    bool found = false;
    while (std::getline(stream, part, L',')) {
        part.erase(0, part.find_first_not_of(L" \t"));
        const size_t end = part.find_last_not_of(L" \t");
        if (end == std::wstring::npos) {
            return false;
        }
        part.erase(end + 1);
        const size_t hyphen = part.find(L'-');
        if (hyphen == std::wstring::npos) {
            if (!IsUnsignedInteger(part, std::numeric_limits<unsigned int>::max())) {
                return false;
            }
        } else {
            if (part.find(L'-', hyphen + 1) != std::wstring::npos) {
                return false;
            }
            const std::wstring first = part.substr(0, hyphen);
            const std::wstring last = part.substr(hyphen + 1);
            if (!IsUnsignedInteger(first, std::numeric_limits<unsigned int>::max()) ||
                !IsUnsignedInteger(last, std::numeric_limits<unsigned int>::max()) ||
                std::stoull(first) > std::stoull(last)) {
                return false;
            }
        }
        found = true;
    }
    return found;
}

void ReaderLoop() {
    std::string pending;
    char buffer[4096];

    while (true) {
        DWORD bytesRead = 0;
        BOOL success = ReadFile(gProcessStdout, buffer, sizeof(buffer), &bytesRead, nullptr);
        if (!success || bytesRead == 0) {
            break;
        }
        pending.append(buffer, buffer + bytesRead);

        size_t newline = 0;
        while ((newline = pending.find('\n')) != std::string::npos) {
            std::string line = pending.substr(0, newline);
            pending.erase(0, newline + 1);
            if (!line.empty() && line.back() == '\r') {
                line.pop_back();
            }
            auto* payload = new std::wstring(Utf8ToWide(line));
            if (!PostMessageW(gMainWindow, WM_APP_LOG_LINE, 0, reinterpret_cast<LPARAM>(payload))) {
                delete payload;
            }
        }
    }

    if (!pending.empty()) {
        auto* payload = new std::wstring(Utf8ToWide(pending));
        if (!PostMessageW(gMainWindow, WM_APP_LOG_LINE, 0, reinterpret_cast<LPARAM>(payload))) {
            delete payload;
        }
    }

    DWORD exitCode = 0;
    if (gProcess) {
        WaitForSingleObject(gProcess, INFINITE);
        GetExitCodeProcess(gProcess, &exitCode);
    }
    PostMessageW(gMainWindow, WM_APP_BACKEND_EXIT, static_cast<WPARAM>(exitCode), 0);
}

void ReleaseExitedBackendHandles() {
    if (gReaderThread.joinable()) {
        gReaderThread.join();
    }
    std::lock_guard<std::mutex> lock(gProcessMutex);
    if (gProcessStdin) {
        CloseHandle(gProcessStdin);
        gProcessStdin = nullptr;
    }
    if (gProcessStdout) {
        CloseHandle(gProcessStdout);
        gProcessStdout = nullptr;
    }
    if (gProcess) {
        CloseHandle(gProcess);
        gProcess = nullptr;
    }
}

bool StartBackendProcess() {
    if (gProcessRunning) {
        return true;
    }
    ReleaseExitedBackendHandles();

    const std::filesystem::path backend = FindBackendExecutable();
    if (backend.empty()) {
        MessageBoxW(gMainWindow,
                    L"veilid_test_node.exe was not found. Build the Rust backend first, then place it beside VeilKnitGui.exe.",
                    kWindowTitle, MB_OK | MB_ICONERROR);
        return false;
    }

    SECURITY_ATTRIBUTES security{};
    security.nLength = sizeof(security);
    security.bInheritHandle = TRUE;

    HANDLE childStdoutRead = nullptr;
    HANDLE childStdoutWrite = nullptr;
    HANDLE childStdinRead = nullptr;
    HANDLE childStdinWrite = nullptr;

    if (!CreatePipe(&childStdoutRead, &childStdoutWrite, &security, 0) ||
        !SetHandleInformation(childStdoutRead, HANDLE_FLAG_INHERIT, 0) ||
        !CreatePipe(&childStdinRead, &childStdinWrite, &security, 0) ||
        !SetHandleInformation(childStdinWrite, HANDLE_FLAG_INHERIT, 0)) {
        MessageBoxW(gMainWindow, L"Could not create the backend communication pipes.",
                    kWindowTitle, MB_OK | MB_ICONERROR);
        if (childStdoutRead) CloseHandle(childStdoutRead);
        if (childStdoutWrite) CloseHandle(childStdoutWrite);
        if (childStdinRead) CloseHandle(childStdinRead);
        if (childStdinWrite) CloseHandle(childStdinWrite);
        return false;
    }

    STARTUPINFOW startup{};
    startup.cb = sizeof(startup);
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdInput = childStdinRead;
    startup.hStdOutput = childStdoutWrite;
    startup.hStdError = childStdoutWrite;

    PROCESS_INFORMATION processInfo{};
    std::wstring commandLine = L"\"" + backend.wstring() + L"\" --gui";
    std::vector<wchar_t> mutableCommand(commandLine.begin(), commandLine.end());
    mutableCommand.push_back(L'\0');
    std::wstring workingDirectory = ExecutableDirectory().wstring();

    BOOL created = CreateProcessW(
        backend.c_str(), mutableCommand.data(), nullptr, nullptr, TRUE,
        CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT, nullptr, workingDirectory.c_str(),
        &startup, &processInfo);

    CloseHandle(childStdoutWrite);
    CloseHandle(childStdinRead);

    if (!created) {
        CloseHandle(childStdoutRead);
        CloseHandle(childStdinWrite);
        MessageBoxW(gMainWindow, L"The Rust backend could not be started.",
                    kWindowTitle, MB_OK | MB_ICONERROR);
        return false;
    }

    CloseHandle(processInfo.hThread);
    gProcess = processInfo.hProcess;
    gProcessStdout = childStdoutRead;
    gProcessStdin = childStdinWrite;
    gProcessRunning = true;
    gAuthenticated = false;
    gReady = false;
    gClosingProperly = false;

    gReaderThread = std::thread(ReaderLoop);
    SetStatus(T(L"Backend started; authenticating..."));
    return true;
}

bool ValidUsername(const std::wstring& username) {
    if (username.empty()) {
        return false;
    }
    return std::all_of(username.begin(), username.end(), [](wchar_t character) {
        return iswalnum(character) || character == L'_' || character == L'-';
    });
}

void SubmitCredentials(bool signup) {
    const std::wstring username = WindowText(gUsernameEdit);
    const std::wstring password = WindowText(gPasswordEdit);

    if (!ValidUsername(username)) {
        MessageBoxW(gMainWindow,
                    T(L"Usernames may only contain letters, numbers, underscores, and hyphens."),
                    T(L"VeilKnit Daemon"), MB_OK | MB_ICONWARNING);
        SetFocus(gUsernameEdit);
        return;
    }
    if (password.empty() || password.find_first_of(L"\r\n") != std::wstring::npos) {
        MessageBoxW(gMainWindow, T(L"Enter a password without line breaks."),
                    T(L"VeilKnit Daemon"), MB_OK | MB_ICONWARNING);
        SetFocus(gPasswordEdit);
        return;
    }

    if (!StartBackendProcess()) {
        return;
    }

    EnableCredentialControls(false);
    SetStatus(signup ? L"Creating account..." : L"Logging in...");
    const std::wstring payload = (signup ? L"s\n" : L"l\n") + username + L"\n" + password + L"\n";
    if (!WriteBackendBytes(WideToUtf8(payload))) {
        EnableCredentialControls(true);
        SetStatus(L"Could not send credentials to the backend.");
    }
    SetWindowTextW(gPasswordEdit, L"");
}

void StartHandshake(bool statusOnly) {
    if (!RequireReady()) {
        return;
    }
    std::wstring key = WindowText(gHandshakeEdit);
    key.erase(0, key.find_first_not_of(L" \t\r\n"));
    const size_t end = key.find_last_not_of(L" \t\r\n");
    if (end != std::wstring::npos) {
        key.erase(end + 1);
    }
    if (!LooksLikeRecordKey(key)) {
        MessageBoxW(gMainWindow, L"Paste a VLD0: DHT record key first.", kWindowTitle,
                    MB_OK | MB_ICONWARNING);
        SetFocus(gHandshakeEdit);
        return;
    }

    SendBackendLine(statusOnly ? L"K" : L"H");
    SendBackendLine(key);
    SetWindowTextW(gHandshakeResultLabel,
                   statusOnly ? L"Requested handshake status..." : L"Handshake request sent...");
}

void SaveSessionLog() {
    if (!RequireReady()) {
        return;
    }
    SendBackendLine(L"U");
    SendBackendLine(L"");
}

void SendSimpleCommand(const wchar_t* command) {
    if (RequireReady()) {
        SendBackendLine(command);
    }
}

bool ReadWalkSettingsFields(std::vector<std::wstring>& values) {
    HWND edits[] = {
        gWalkNormalMinHopsEdit, gWalkNormalMaxHopsEdit,
        gWalkNormalMinSecsEdit, gWalkNormalTargetSecsEdit, gWalkNormalMaxSecsEdit,
        gWalkMailMinHopsEdit, gWalkMailMaxHopsEdit,
        gWalkMailMinSecsEdit, gWalkMailTargetSecsEdit, gWalkMailMaxSecsEdit,
    };
    values.clear();
    values.reserve(11);
    for (HWND edit : edits) {
        const std::wstring value = WindowText(edit);
        if (!IsUnsignedInteger(value)) {
            return false;
        }
        values.push_back(value);
    }
    values.push_back(
        SendMessageW(gWalkMailModeCheckbox, BM_GETCHECK, 0, 0) == BST_CHECKED ? L"1" : L"0");
    return true;
}

void ApplyWalkSettings() {
    std::vector<std::wstring> values;
    if (!ReadWalkSettingsFields(values)) {
        MessageBoxW(
            gMainWindow,
            L"Every hop and interval field must contain a non-negative whole number. Intervals are in seconds.",
            kWindowTitle, MB_OK | MB_ICONWARNING);
        return;
    }

    std::wstring command = L"walk-set";
    for (const std::wstring& value : values) {
        command += L" ";
        command += value;
    }
    SendBackendLine(command);
}

void HandleNetworkAction(int id) {
    if (!RequireReady()) {
        return;
    }
    switch (id) {
    case ID_WALK_APPLY:
        ApplyWalkSettings();
        break;
    case ID_WALK_NORMAL_START:
        SendBackendLine(L"walk-normal");
        break;
    case ID_WALK_MAIL_START:
        SendBackendLine(L"walk-mail");
        break;
    case ID_WALK_STATUS:
        SendBackendLine(L"walk-settings");
        SendBackendLine(L"P");
        break;
    case ID_WALK_STOP: SendBackendLine(L"O"); break;
    case ID_ROUTE_STATUS: SendBackendLine(L"C"); break;
    case ID_NODE_LIST: SendBackendLine(L"I"); break;
    case ID_DAEMON_STATUS: SendBackendLine(L"D"); break;
    default: break;
    }
}

bool ParseDhtGroups(const std::wstring& input, std::vector<std::wstring>& groups) {
    std::wstringstream stream(input);
    std::wstring part;
    while (std::getline(stream, part, L',')) {
        part.erase(0, part.find_first_not_of(L" \t"));
        const size_t end = part.find_last_not_of(L" \t");
        if (end != std::wstring::npos) part.erase(end + 1);
        if (part.empty() || !IsUnsignedInteger(part)) {
            return false;
        }
        unsigned long value = 0;
        try {
            value = std::stoul(part);
        } catch (...) {
            return false;
        }
        if (value < 1 || value > 250) {
            return false;
        }
        groups.push_back(std::to_wstring(value));
    }
    return !groups.empty() && groups.size() <= 250;
}

void HandleDhtAction(int id) {
    if (!RequireReady()) {
        return;
    }

    const std::wstring index = WindowText(gDhtIndexEdit);
    const std::wstring subkey = WindowText(gDhtSubkeyEdit);
    switch (id) {
    case ID_DHT_CREATE: {
        const std::wstring name = WindowText(gDhtNameEdit);
        std::vector<std::wstring> groups;
        if (name.empty() || !IsSingleLine(name) || !ParseDhtGroups(WindowText(gDhtGroupsEdit), groups)) {
            MessageBoxW(gMainWindow,
                        L"Enter a single-line DHT name and comma-separated owner group sizes from 1 to 250.",
                        kWindowTitle, MB_OK | MB_ICONWARNING);
            return;
        }
        std::vector<std::wstring> lines{L"N", name};
        for (size_t groupIndex = 0; groupIndex < groups.size(); ++groupIndex) {
            lines.push_back(groups[groupIndex]);
            if (groupIndex + 1 < groups.size()) {
                lines.push_back(L"y");
            } else if (groups.size() < 250) {
                lines.push_back(L"n");
            }
        }
        SendBackendLines(lines);
        break;
    }
    case ID_DHT_INSPECT:
        if (IsUnsignedInteger(index, std::numeric_limits<size_t>::max())) SendBackendLines({L"G", index});
        else MessageBoxW(gMainWindow, L"Enter a numeric DHT index.", kWindowTitle, MB_OK | MB_ICONWARNING);
        break;
    case ID_DHT_WRITE: {
        const std::wstring data = WindowText(gDhtDataEdit);
        if (!IsUnsignedInteger(index, std::numeric_limits<size_t>::max()) || !IsUnsignedInteger(subkey, std::numeric_limits<unsigned int>::max()) || !IsSingleLine(data)) {
            MessageBoxW(gMainWindow, L"Enter an index, subkey, and single-line value.",
                        kWindowTitle, MB_OK | MB_ICONWARNING);
            return;
        }
        SendBackendLines({L"W", index, subkey, data});
        break;
    }
    case ID_DHT_READ:
        if (IsUnsignedInteger(index, std::numeric_limits<size_t>::max()) && IsUnsignedInteger(subkey, std::numeric_limits<unsigned int>::max())) SendBackendLines({L"R", index, subkey});
        else MessageBoxW(gMainWindow, L"Enter numeric DHT index and subkey values.", kWindowTitle, MB_OK | MB_ICONWARNING);
        break;
    case ID_DHT_READ_ALL:
        if (IsUnsignedInteger(index, std::numeric_limits<size_t>::max())) SendBackendLines({L"L", index});
        else MessageBoxW(gMainWindow, L"Enter a numeric DHT index.", kWindowTitle, MB_OK | MB_ICONWARNING);
        break;
    case ID_DHT_SAVE:
        SendBackendLine(L"S");
        break;
    case ID_DHT_EXTERNAL_SELECTED: {
        const std::wstring key = WindowText(gDhtExternalKeyEdit);
        const std::wstring locations = WindowText(gDhtLocationsEdit);
        if (!LooksLikeRecordKey(key) || !LooksLikeSubkeySelection(locations)) {
            MessageBoxW(gMainWindow, L"Enter an external VLD0 key and subkeys such as 0,1,10,50-75.",
                        kWindowTitle, MB_OK | MB_ICONWARNING);
            return;
        }
        SendBackendLines({L"Y", key, locations});
        break;
    }
    case ID_DHT_EXTERNAL_ALL: {
        const std::wstring key = WindowText(gDhtExternalKeyEdit);
        if (!LooksLikeRecordKey(key)) {
            MessageBoxW(gMainWindow, L"Enter an external VLD0 key.",
                        kWindowTitle, MB_OK | MB_ICONWARNING);
            return;
        }
        SendBackendLines({L"X", key});
        break;
    }
    default: break;
    }
}

void HandleMailboxAction(int id) {
    if (!RequireReady()) {
        return;
    }
    switch (id) {
    case ID_MAIL_SEND: {
        const std::wstring recipient = WindowText(gMailRecipientEdit);
        const std::wstring app = WindowText(gMailAppEdit);
        const std::wstring payload = WindowText(gMailPayloadEdit);
        if (!LooksLikeRecordKey(recipient) || app.empty() || !IsSingleLine(app) || !IsSingleLine(payload)) {
            MessageBoxW(gMainWindow,
                        L"Enter a recipient VLD0 key, application id, and single-line payload.",
                        kWindowTitle, MB_OK | MB_ICONWARNING);
            return;
        }
        SendBackendLines({L"mail send", recipient, app, payload});
        break;
    }
    case ID_MAIL_STATUS: SendBackendLine(L"mail status"); break;
    case ID_MAIL_LIST: SendBackendLine(L"mail list"); break;
    case ID_MAIL_RETRIEVE: SendBackendLine(L"mail retrieve"); break;
    case ID_MAIL_STATS: SendBackendLine(L"mail stats"); break;
    case ID_MAIL_FLUSH: SendBackendLine(L"mail flush"); break;
    case ID_MAIL_REPAIR: SendBackendLine(L"mail repair"); break;
    default: break;
    }
}

void HandleApplicationAction(int id) {
    if (!RequireReady()) {
        return;
    }
    const std::wstring appId = WindowText(gAppIdEdit);
    switch (id) {
    case ID_APP_ADD: {
        const std::wstring name = WindowText(gAppNameEdit);
        if (appId.empty() || name.empty() || !IsSingleLine(appId) || !IsSingleLine(name)) {
            MessageBoxW(gMainWindow, L"Enter an application id and display name.",
                        kWindowTitle, MB_OK | MB_ICONWARNING);
            return;
        }
        SendBackendLines({L"app-add", appId, name});
        break;
    }
    case ID_APP_LIST: SendBackendLine(L"app-list"); break;
    case ID_APP_ROTATE:
        if (!appId.empty()) SendBackendLines({L"app-rotate", appId});
        break;
    case ID_APP_PENDING: SendBackendLine(L"app-pending"); break;
    case ID_APP_APPROVE: {
        const std::wstring request = WindowText(gAppRequestEdit);
        if (!request.empty()) SendBackendLine(L"app-approve " + request);
        break;
    }
    case ID_APP_REJECT: {
        const std::wstring request = WindowText(gAppRequestEdit);
        std::wstring reason = WindowText(gAppReasonEdit);
        if (reason.empty()) reason = L"rejected by the local user";
        if (!request.empty() && IsSingleLine(reason)) {
            SendBackendLine(L"app-reject " + request + L" " + reason);
        }
        break;
    }
    default: break;
    }
}

bool CopyTextToClipboard(const std::wstring& value) {
    if (!OpenClipboard(gMainWindow)) {
        return false;
    }
    EmptyClipboard();
    const size_t bytes = (value.size() + 1) * sizeof(wchar_t);
    HGLOBAL memory = GlobalAlloc(GMEM_MOVEABLE, bytes);
    bool copied = false;
    if (memory) {
        void* destination = GlobalLock(memory);
        if (destination) {
            memcpy(destination, value.c_str(), bytes);
            GlobalUnlock(memory);
            copied = SetClipboardData(CF_UNICODETEXT, memory) != nullptr;
        }
        if (!copied) {
            GlobalFree(memory);
        }
    }
    CloseClipboard();
    return copied;
}

void CopyMainKey() {
    if (gMainDhtKey.empty()) {
        MessageBoxW(gMainWindow, L"The main DHT record key is not available yet.",
                    kWindowTitle, MB_OK | MB_ICONINFORMATION);
        return;
    }
    CopyTextToClipboard(gMainDhtKey);
}

void HandleHeaderAction(int id) {
    if (!RequireReady()) {
        return;
    }
    switch (id) {
    case ID_HEADERS_REFRESH:
        SendBackendLine(L"headers");
        break;
    case ID_HEADERS_COPY_MAIN: {
        const std::wstring value = WindowText(gMainHeaderEdit);
        if (value.empty()) {
            MessageBoxW(gMainWindow, L"The main header has not been read yet.",
                        kWindowTitle, MB_OK | MB_ICONINFORMATION);
        } else {
            CopyTextToClipboard(value);
        }
        break;
    }
    case ID_HEADERS_COPY_MAILBOX: {
        const std::wstring value = WindowText(gMailboxHeaderEdit);
        if (value.empty()) {
            MessageBoxW(gMainWindow, L"The mailbox header has not been read yet.",
                        kWindowTitle, MB_OK | MB_ICONINFORMATION);
        } else {
            CopyTextToClipboard(value);
        }
        break;
    }
    default: break;
    }
}

void RemoveTrayIcon() {
    if (gTrayAdded) {
        Shell_NotifyIconW(NIM_DELETE, &gTrayData);
        gTrayAdded = false;
    }
}

void AddTrayIcon() {
    if (gTrayAdded) {
        return;
    }
    ZeroMemory(&gTrayData, sizeof(gTrayData));
    gTrayData.cbSize = sizeof(gTrayData);
    gTrayData.hWnd = gMainWindow;
    gTrayData.uID = 1;
    gTrayData.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    gTrayData.uCallbackMessage = WM_APP_TRAY;
    gTrayData.hIcon = static_cast<HICON>(LoadImageW(
        gInstance,
        MAKEINTRESOURCEW(101),
        IMAGE_ICON,
        GetSystemMetrics(SM_CXSMICON),
        GetSystemMetrics(SM_CYSMICON),
        LR_DEFAULTCOLOR | LR_SHARED));
    if (!gTrayData.hIcon) gTrayData.hIcon = LoadIconW(nullptr, IDI_APPLICATION);
    wcscpy_s(gTrayData.szTip, L"VeilKnit Daemon");
    if (Shell_NotifyIconW(NIM_ADD, &gTrayData)) {
        gTrayData.uVersion = NOTIFYICON_VERSION_4;
        Shell_NotifyIconW(NIM_SETVERSION, &gTrayData);
        gTrayAdded = true;
    }
}

void RestoreFromTray() {
    RemoveTrayIcon();
    ShowWindow(gMainWindow, SW_RESTORE);
    SetForegroundWindow(gMainWindow);
}

void ShowTrayMenu() {
    POINT cursor{};
    GetCursorPos(&cursor);
    HMENU menu = CreatePopupMenu();
    AppendMenuW(menu, MF_STRING, ID_TRAY_OPEN, L"Open VeilKnit Daemon");
    AppendMenuW(menu, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(menu, MF_STRING, ID_TRAY_PROPER, L"Close properly");
    AppendMenuW(menu, MF_STRING, ID_TRAY_CRAZY, L"Close like a crazy person");
    SetForegroundWindow(gMainWindow);
    TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_LEFTALIGN,
                   cursor.x, cursor.y, 0, gMainWindow, nullptr);
    DestroyMenu(menu);
}

void ForceClose() {
    gClosingProperly = false;
    if (gProcessRunning && gProcess) {
        TerminateProcess(gProcess, 0xDEAD);
        WaitForSingleObject(gProcess, 2000);
    }
    DestroyWindow(gMainWindow);
}

void BeginProperShutdown() {
    if (gClosingProperly.exchange(true)) {
        return;
    }

    SetStatus(L"Closing properly: saving state and stopping services...");
    EnableWindow(gShutdownButton, FALSE);
    EnableWindow(gHandshakeStartButton, FALSE);
    EnableWindow(gHandshakeStatusButton, FALSE);

    if (!gProcessRunning) {
        DestroyWindow(gMainWindow);
        return;
    }

    if (!gAuthenticated) {
        // No account session exists yet, so there is no daemon state to save.
        ForceClose();
        return;
    }

    if (!SendBackendLine(L"Q")) {
        ForceClose();
    }
}

int LoadSavedCloseAction() {
    DWORD value = 0;
    DWORD size = sizeof(value);
    RegGetValueW(HKEY_CURRENT_USER, L"Software\\VeilKnit\\Daemon", L"CloseAction",
                 RRF_RT_REG_DWORD, nullptr, &value, &size);
    return value >= 1 && value <= 3 ? static_cast<int>(value) : 0;
}

void SaveCloseAction(int action) {
    HKEY key{};
    if (RegCreateKeyExW(HKEY_CURRENT_USER, L"Software\\VeilKnit\\Daemon", 0, nullptr, 0,
                        KEY_SET_VALUE, nullptr, &key, nullptr) == ERROR_SUCCESS) {
        DWORD value = static_cast<DWORD>(action);
        RegSetValueExW(key, L"CloseAction", 0, REG_DWORD,
                       reinterpret_cast<const BYTE*>(&value), sizeof(value));
        RegCloseKey(key);
    }
    gSavedCloseAction = action;
    if (gRememberCloseCheckbox) {
        SendMessageW(gRememberCloseCheckbox, BM_SETCHECK, action ? BST_CHECKED : BST_UNCHECKED, 0);
    }
}

void MinimizeWindowToTray() {
    AddTrayIcon();
    ShowWindow(gMainWindow, SW_HIDE);
}

void PromptForClose() {
    if (gSavedCloseAction == 1) { BeginProperShutdown(); return; }
    if (gSavedCloseAction == 2) { ForceClose(); return; }
    if (gSavedCloseAction == 3) { MinimizeWindowToTray(); return; }

    TASKDIALOG_BUTTON buttons[] = {
        {ID_CLOSE_PROPER, L"Close properly\nSave DHT state and stop network services cleanly."},
        {ID_CLOSE_TRAY, L"Minimize to tray\nKeep the daemon running in the notification area."},
        {ID_CLOSE_CRAZY, L"Close like a crazy person\nTerminate immediately without waiting for cleanup."},
    };

    TASKDIALOGCONFIG config{};
    config.cbSize = sizeof(config);
    config.hwndParent = gMainWindow;
    config.dwFlags = TDF_USE_COMMAND_LINKS | TDF_POSITION_RELATIVE_TO_WINDOW;
    config.dwCommonButtons = TDCBF_CANCEL_BUTTON;
    config.pszWindowTitle = kWindowTitle;
    config.pszMainInstruction = L"What should the X button do?";
    config.pszContent = L"The proper option saves daemon state before networking services stop.";
    config.pszVerificationText = L"Always do this when I click X";
    config.pButtons = buttons;
    config.cButtons = ARRAYSIZE(buttons);
    config.nDefaultButton = ID_CLOSE_PROPER;
    config.pszMainIcon = TD_WARNING_ICON;

    int selected = 0;
    BOOL remember = FALSE;
    HRESULT result = TaskDialogIndirect(&config, &selected, nullptr, &remember);
    if (SUCCEEDED(result)) {
        if (remember && (selected == ID_CLOSE_PROPER || selected == ID_CLOSE_CRAZY || selected == ID_CLOSE_TRAY)) {
            SaveCloseAction(selected == ID_CLOSE_PROPER ? 1 : selected == ID_CLOSE_CRAZY ? 2 : 3);
        }
        if (selected == ID_CLOSE_PROPER) BeginProperShutdown();
        else if (selected == ID_CLOSE_CRAZY) ForceClose();
        else if (selected == ID_CLOSE_TRAY) MinimizeWindowToTray();
        return;
    }

    int fallback = MessageBoxW(gMainWindow,
                               L"Yes = close properly\nNo = minimize to tray\nCancel = keep running",
                               kWindowTitle, MB_YESNOCANCEL | MB_ICONWARNING);
    if (fallback == IDYES) BeginProperShutdown();
    if (fallback == IDNO) MinimizeWindowToTray();
}

void LayoutPages(int clientWidth, int clientHeight) {
    constexpr int headerHeight = 54;
    MoveWindow(gHeaderTitle, 16, 8, clientWidth / 2, 28, TRUE);
    MoveWindow(gStatusLabel, clientWidth / 2, 14, clientWidth / 2 - 18, 22, TRUE);
    MoveWindow(gTab, 8, headerHeight, clientWidth - 16, clientHeight - headerHeight - 8, TRUE);

    RECT pageRect{};
    GetClientRect(gTab, &pageRect);
    TabCtrl_AdjustRect(gTab, FALSE, &pageRect);
    const int pageWidth = pageRect.right - pageRect.left;
    const int pageHeight = pageRect.bottom - pageRect.top;
    for (const Page& page : gPages) {
        MoveWindow(page.container, pageRect.left, pageRect.top, pageWidth, pageHeight, TRUE);
    }

    if (gPages.size() < 8) {
        return;
    }

    const int padding = 14;
    const int buttonWidth = 112;
    const int rowHeight = 28;

    MoveWindow(gUsernameLabel, padding, 17, 76, 22, TRUE);
    MoveWindow(gUsernameEdit, 86, 12, 180, rowHeight, TRUE);
    MoveWindow(gLoginButton, 280, 11, 86, 30, TRUE);
    MoveWindow(gSignupButton, 374, 11, 92, 30, TRUE);
    MoveWindow(gHelpButton, 474, 11, 72, 30, TRUE);
    MoveWindow(gPasswordLabel, padding, 53, 74, 22, TRUE);
    MoveWindow(gPasswordEdit, 86, 48, 180, rowHeight, TRUE);
    MoveWindow(gLanguageLabel, 280, 53, 78, 22, TRUE);
    MoveWindow(gLanguageCombo, 360, 48, 186, 180, TRUE);

    MoveWindow(gMainKeyLabel, padding, 89, 112, 22, TRUE);
    MoveWindow(gMainKeyEdit, 126, 84, std::max(120, pageWidth - 126 - buttonWidth - 28), rowHeight, TRUE);
    MoveWindow(gCopyKeyButton, pageWidth - buttonWidth - padding, 83, buttonWidth, 30, TRUE);

    MoveWindow(gMinimizeCheckbox, padding, 125, 238, 24, TRUE);
    MoveWindow(gRememberCloseCheckbox, padding, 151, 260, 24, TRUE);
    MoveWindow(gSaveLogButton, 280, 146, 118, 30, TRUE);
    MoveWindow(gShutdownButton, 408, 146, 146, 30, TRUE);
    MoveWindow(gPages[0].log, padding, 184, pageWidth - padding * 2,
               std::max(40, pageHeight - 198), TRUE);

    MoveWindow(gHandshakeLabel, padding, 18, 132, 22, TRUE);
    MoveWindow(gHandshakeEdit, 146, 13, std::max(120, pageWidth - 146 - 306), rowHeight, TRUE);
    MoveWindow(gHandshakeStartButton, pageWidth - 292, 12, 136, 30, TRUE);
    MoveWindow(gHandshakeStatusButton, pageWidth - 148, 12, 134, 30, TRUE);
    MoveWindow(gHandshakeResultLabel, padding, 51, pageWidth - padding * 2, 22, TRUE);
    MoveWindow(gPages[1].log, padding, 80, pageWidth - padding * 2,
               std::max(40, pageHeight - 94), TRUE);

    // Adaptive walk controls. Keep each mode on two rows so the controls remain
    // usable at the minimum supported window width.
    const int modeLabelWidth = 82;
    const int fieldLabelWidth = 68;
    const int fieldEditWidth = 68;
    const int fieldGap = 12;
    const int fieldStart = padding + modeLabelWidth;

    auto layoutHopRow = [&](int y, HWND modeLabel, HWND minLabel, HWND minEdit,
                            HWND maxLabel, HWND maxEdit) {
        MoveWindow(modeLabel, padding, y + 5, modeLabelWidth - 6, 22, TRUE);
        int x = fieldStart;
        MoveWindow(minLabel, x, y + 5, fieldLabelWidth, 22, TRUE); x += fieldLabelWidth;
        MoveWindow(minEdit, x, y, fieldEditWidth, rowHeight, TRUE); x += fieldEditWidth + fieldGap;
        MoveWindow(maxLabel, x, y + 5, fieldLabelWidth, 22, TRUE); x += fieldLabelWidth;
        MoveWindow(maxEdit, x, y, fieldEditWidth, rowHeight, TRUE);
    };
    auto layoutIntervalRow = [&](int y, HWND minLabel, HWND minEdit,
                                 HWND targetLabel, HWND targetEdit,
                                 HWND maxLabel, HWND maxEdit) {
        int x = fieldStart;
        MoveWindow(minLabel, x, y + 5, fieldLabelWidth, 22, TRUE); x += fieldLabelWidth;
        MoveWindow(minEdit, x, y, fieldEditWidth, rowHeight, TRUE); x += fieldEditWidth + fieldGap;
        MoveWindow(targetLabel, x, y + 5, fieldLabelWidth, 22, TRUE); x += fieldLabelWidth;
        MoveWindow(targetEdit, x, y, fieldEditWidth, rowHeight, TRUE); x += fieldEditWidth + fieldGap;
        MoveWindow(maxLabel, x, y + 5, fieldLabelWidth, 22, TRUE); x += fieldLabelWidth;
        MoveWindow(maxEdit, x, y, std::max(48, std::min(fieldEditWidth, pageWidth - x - padding)), rowHeight, TRUE);
    };

    layoutHopRow(12, gWalkNormalLabel, gWalkNormalMinHopsLabel, gWalkNormalMinHopsEdit,
                 gWalkNormalMaxHopsLabel, gWalkNormalMaxHopsEdit);
    layoutIntervalRow(46, gWalkNormalMinSecsLabel, gWalkNormalMinSecsEdit,
                      gWalkNormalTargetSecsLabel, gWalkNormalTargetSecsEdit,
                      gWalkNormalMaxSecsLabel, gWalkNormalMaxSecsEdit);

    layoutHopRow(82, gWalkMailLabel, gWalkMailMinHopsLabel, gWalkMailMinHopsEdit,
                 gWalkMailMaxHopsLabel, gWalkMailMaxHopsEdit);
    layoutIntervalRow(116, gWalkMailMinSecsLabel, gWalkMailMinSecsEdit,
                      gWalkMailTargetSecsLabel, gWalkMailTargetSecsEdit,
                      gWalkMailMaxSecsLabel, gWalkMailMaxSecsEdit);

    MoveWindow(gWalkMailModeCheckbox, padding, 151, 238, 24, TRUE);
    const int actionGap = 8;
    const int threeButtonWidth = std::max(96, (pageWidth - padding * 2 - actionGap * 2) / 3);
    int actionX = padding;
    MoveWindow(gWalkApplyButton, actionX, 180, threeButtonWidth, 30, TRUE);
    actionX += threeButtonWidth + actionGap;
    MoveWindow(gWalkNormalStartButton, actionX, 180, threeButtonWidth, 30, TRUE);
    actionX += threeButtonWidth + actionGap;
    MoveWindow(gWalkMailStartButton, actionX, 180,
               std::max(96, pageWidth - padding - actionX), 30, TRUE);

    const int fiveButtonWidth = std::max(76, (pageWidth - padding * 2 - actionGap * 4) / 5);
    actionX = padding;
    MoveWindow(gWalkStatusButton, actionX, 218, fiveButtonWidth, 30, TRUE);
    actionX += fiveButtonWidth + actionGap;
    MoveWindow(gWalkStopButton, actionX, 218, fiveButtonWidth, 30, TRUE);
    actionX += fiveButtonWidth + actionGap;
    MoveWindow(gRouteStatusButton, actionX, 218, fiveButtonWidth, 30, TRUE);
    actionX += fiveButtonWidth + actionGap;
    MoveWindow(gNodeListButton, actionX, 218, fiveButtonWidth, 30, TRUE);
    actionX += fiveButtonWidth + actionGap;
    MoveWindow(gDaemonStatusButton, actionX, 218,
               std::max(76, pageWidth - padding - actionX), 30, TRUE);
    MoveWindow(gPages[2].log, padding, 258, pageWidth - padding * 2,
               std::max(40, pageHeight - 272), TRUE);

    // Own public headers.
    const int headerBoxHeight = std::max(80, (pageHeight - 190) / 2);
    MoveWindow(gMainHeaderLabel, padding, 17, 250, 22, TRUE);
    MoveWindow(gRefreshHeadersButton, pageWidth - 142, 10, 128, 30, TRUE);
    MoveWindow(gCopyMainHeaderButton, pageWidth - 290, 10, 140, 30, TRUE);
    MoveWindow(gMainHeaderEdit, padding, 42, pageWidth - padding * 2, headerBoxHeight, TRUE);

    const int mailboxHeaderY = 52 + headerBoxHeight;
    MoveWindow(gMailboxHeaderLabel, padding, mailboxHeaderY, 300, 22, TRUE);
    MoveWindow(gCopyMailboxHeaderButton, pageWidth - 174, mailboxHeaderY - 7, 160, 30, TRUE);
    MoveWindow(gMailboxHeaderEdit, padding, mailboxHeaderY + 26,
               pageWidth - padding * 2, headerBoxHeight, TRUE);

    const int headerLogY = mailboxHeaderY + 36 + headerBoxHeight;
    MoveWindow(gPages[3].log, padding, headerLogY, pageWidth - padding * 2,
               std::max(36, pageHeight - headerLogY - padding), TRUE);

    MoveWindow(gDhtNameLabel, padding, 18, 40, 22, TRUE);
    MoveWindow(gDhtNameEdit, 58, 13, 150, rowHeight, TRUE);
    MoveWindow(gDhtGroupsLabel, 218, 18, 88, 22, TRUE);
    MoveWindow(gDhtGroupsEdit, 310, 13, 90, rowHeight, TRUE);
    MoveWindow(gDhtCreateButton, 410, 12, 108, 30, TRUE);
    MoveWindow(gDhtIndexLabel, padding, 54, 40, 22, TRUE);
    MoveWindow(gDhtIndexEdit, 58, 49, 48, rowHeight, TRUE);
    MoveWindow(gDhtSubkeyLabel, 116, 54, 55, 22, TRUE);
    MoveWindow(gDhtSubkeyEdit, 171, 49, 48, rowHeight, TRUE);
    MoveWindow(gDhtDataLabel, 229, 54, 45, 22, TRUE);
    MoveWindow(gDhtDataEdit, 276, 49, std::max(90, pageWidth - 290), rowHeight, TRUE);
    MoveWindow(gDhtInspectButton, padding, 84, 82, 30, TRUE);
    MoveWindow(gDhtWriteButton, 104, 84, 76, 30, TRUE);
    MoveWindow(gDhtReadButton, 188, 84, 76, 30, TRUE);
    MoveWindow(gDhtReadAllButton, 272, 84, 88, 30, TRUE);
    MoveWindow(gDhtSaveButton, 368, 84, 98, 30, TRUE);
    MoveWindow(gDhtExternalKeyLabel, padding, 126, 104, 22, TRUE);
    MoveWindow(gDhtExternalKeyEdit, 120, 121, std::max(120, pageWidth - 134), rowHeight, TRUE);
    MoveWindow(gDhtLocationsLabel, padding, 162, 64, 22, TRUE);
    MoveWindow(gDhtLocationsEdit, 80, 157, 158, rowHeight, TRUE);
    MoveWindow(gDhtExternalSelectedButton, 248, 156, 114, 30, TRUE);
    MoveWindow(gDhtExternalAllButton, 370, 156, 132, 30, TRUE);
    MoveWindow(gPages[4].log, padding, 198, pageWidth - padding * 2,
               std::max(40, pageHeight - 212), TRUE);

    MoveWindow(gMailRecipientLabel, padding, 18, 108, 22, TRUE);
    MoveWindow(gMailRecipientEdit, 126, 13, std::max(120, pageWidth - 336), rowHeight, TRUE);
    MoveWindow(gMailAppLabel, pageWidth - 200, 18, 74, 22, TRUE);
    MoveWindow(gMailAppEdit, pageWidth - 120, 13, 106, rowHeight, TRUE);
    MoveWindow(gMailPayloadLabel, padding, 54, 58, 22, TRUE);
    MoveWindow(gMailPayloadEdit, 76, 49, std::max(120, pageWidth - 204), rowHeight, TRUE);
    MoveWindow(gMailSendButton, pageWidth - 118, 48, 104, 30, TRUE);
    MoveWindow(gMailStatusButton, padding, 84, 86, 30, TRUE);
    MoveWindow(gMailListButton, 108, 84, 86, 30, TRUE);
    MoveWindow(gMailRetrieveButton, 202, 84, 86, 30, TRUE);
    MoveWindow(gMailStatsButton, 296, 84, 86, 30, TRUE);
    MoveWindow(gMailFlushButton, 390, 84, 86, 30, TRUE);
    MoveWindow(gMailRepairButton, 484, 84, 86, 30, TRUE);
    MoveWindow(gPages[5].log, padding, 124, pageWidth - padding * 2,
               std::max(40, pageHeight - 138), TRUE);

    MoveWindow(gAppIdLabel, padding, 18, 86, 22, TRUE);
    MoveWindow(gAppIdEdit, 104, 13, 146, rowHeight, TRUE);
    MoveWindow(gAppNameLabel, 260, 18, 92, 22, TRUE);
    MoveWindow(gAppNameEdit, 356, 13, std::max(90, pageWidth - 480), rowHeight, TRUE);
    MoveWindow(gAppAddButton, pageWidth - 112, 12, 98, 30, TRUE);
    MoveWindow(gAppListButton, padding, 48, 98, 30, TRUE);
    MoveWindow(gAppRotateButton, 120, 48, 100, 30, TRUE);
    MoveWindow(gAppPendingButton, 228, 48, 132, 30, TRUE);
    MoveWindow(gAppRequestLabel, padding, 85, 72, 22, TRUE);
    MoveWindow(gAppRequestEdit, 90, 80, 58, rowHeight, TRUE);
    MoveWindow(gAppApproveButton, 156, 79, 90, 30, TRUE);
    MoveWindow(gAppReasonLabel, 256, 85, 92, 22, TRUE);
    MoveWindow(gAppReasonEdit, 352, 80, std::max(80, pageWidth - 470), rowHeight, TRUE);
    MoveWindow(gAppRejectButton, pageWidth - 110, 79, 96, 30, TRUE);
    MoveWindow(gPages[6].log, padding, 122, pageWidth - padding * 2,
               std::max(40, pageHeight - 136), TRUE);

    MoveWindow(gPages[7].log, padding, padding, pageWidth - padding * 2,
               pageHeight - padding * 2, TRUE);
}

void ShowSelectedPage() {
    const int selected = TabCtrl_GetCurSel(gTab);
    for (size_t index = 0; index < gPages.size(); ++index) {
        ShowWindow(gPages[index].container, static_cast<int>(index) == selected ? SW_SHOW : SW_HIDE);
    }
}

void DrawOwnerControl(const DRAWITEMSTRUCT* draw) {
    if (draw->CtlID == ID_TAB) {
        wchar_t text[64]{};
        TCITEMW item{};
        item.mask = TCIF_TEXT;
        item.pszText = text;
        item.cchTextMax = ARRAYSIZE(text);
        TabCtrl_GetItem(gTab, draw->itemID, &item);

        const bool selected = (draw->itemState & ODS_SELECTED) != 0;
        HBRUSH background = CreateSolidBrush(selected ? kColorRed : kColorPanel);
        FillRect(draw->hDC, &draw->rcItem, background);
        DeleteObject(background);
        SetBkMode(draw->hDC, TRANSPARENT);
        SetTextColor(draw->hDC, kColorText);
        SelectObject(draw->hDC, gUiFont);
        RECT textRect = draw->rcItem;
        DrawTextW(draw->hDC, text, -1, &textRect, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        return;
    }

    const bool disabled = (draw->itemState & ODS_DISABLED) != 0;
    const bool pressed = (draw->itemState & ODS_SELECTED) != 0;
    COLORREF fill = disabled ? kColorRedDark : (pressed ? kColorRedDark : kColorRed);
    HBRUSH background = CreateSolidBrush(fill);
    FillRect(draw->hDC, &draw->rcItem, background);
    DeleteObject(background);
    FrameRect(draw->hDC, &draw->rcItem, reinterpret_cast<HBRUSH>(GetStockObject(BLACK_BRUSH)));

    wchar_t text[128]{};
    GetWindowTextW(draw->hwndItem, text, ARRAYSIZE(text));
    SetBkMode(draw->hDC, TRANSPARENT);
    SetTextColor(draw->hDC, disabled ? kColorMuted : kColorText);
    SelectObject(draw->hDC, gUiFont);
    RECT textRect = draw->rcItem;
    if (pressed) OffsetRect(&textRect, 1, 1);
    DrawTextW(draw->hDC, text, -1, &textRect, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
}

void CreateInterface(HWND window) {
    gHeaderTitle = CreateLabel(window, L"VEILKNIT DAEMON");
    ApplyFont(gHeaderTitle, gTitleFont);
    gStatusLabel = CreateLabel(window, L"Not connected");

    gTab = CreateWindowExW(0, WC_TABCONTROLW, L"",
                           WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_TABSTOP |
                               TCS_OWNERDRAWFIXED | TCS_FIXEDWIDTH | TCS_MULTILINE,
                           0, 0, 10, 10, window, reinterpret_cast<HMENU>(static_cast<INT_PTR>(ID_TAB)), gInstance, nullptr);
    ApplyFont(gTab);
    SetWindowTheme(gTab, L"", L"");
    SendMessageW(gTab, TCM_SETITEMSIZE, 0, MAKELPARAM(112, 30));

    const wchar_t* labels[] = {
        L"Overview", L"Handshake", L"Network", L"Headers",
        L"DHT", L"Mailbox", L"Applications", L"All Logs"
    };

    for (int index = 0; index < static_cast<int>(ARRAYSIZE(labels)); ++index) {
        TCITEMW item{};
        item.mask = TCIF_TEXT;
        item.pszText = const_cast<wchar_t*>(labels[index]);
        TabCtrl_InsertItem(gTab, index, &item);

        HWND page = CreateWindowExW(0, kPageClass, L"", WS_CHILD | WS_CLIPCHILDREN,
                                    0, 0, 10, 10, gTab, nullptr, gInstance, nullptr);
        Page info{page, CreateLog(page)};
        gPages.push_back(info);
    }

    HWND overview = gPages[0].container;
    gUsernameLabel = CreateLabel(overview, L"Username");
    gUsernameEdit = CreateEdit(overview, ID_USERNAME);
    gPasswordLabel = CreateLabel(overview, L"Password");
    gPasswordEdit = CreateEdit(overview, ID_PASSWORD, ES_PASSWORD);
    SendMessageW(gPasswordEdit, EM_SETPASSWORDCHAR, 0x25CF, 0);
    gLoginButton = CreateButton(overview, L"Login", ID_LOGIN);
    gSignupButton = CreateButton(overview, L"Sign up", ID_SIGNUP);
    gMainKeyLabel = CreateLabel(overview, L"Main DHT key");
    gMainKeyEdit = CreateEdit(overview, ID_MAIN_KEY, ES_READONLY);
    gCopyKeyButton = CreateButton(overview, L"Copy key", ID_COPY_KEY);
    gMinimizeCheckbox = CreateWindowExW(0, L"BUTTON", L"Minimize to notification area",
                                         WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX,
                                         0, 0, 10, 10, overview,
                                         reinterpret_cast<HMENU>(static_cast<INT_PTR>(ID_MINIMIZE_TO_TRAY)), gInstance, nullptr);
    ApplyFont(gMinimizeCheckbox);
    SendMessageW(gMinimizeCheckbox, BM_SETCHECK, BST_CHECKED, 0);
    SetWindowTheme(gMinimizeCheckbox, L"DarkMode_Explorer", nullptr);
    gRememberCloseCheckbox = CreateWindowExW(0, L"BUTTON", L"Always use saved X-button action",
                                         WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX,
                                         0, 0, 10, 10, overview,
                                         reinterpret_cast<HMENU>(static_cast<INT_PTR>(ID_REMEMBER_CLOSE)), gInstance, nullptr);
    ApplyFont(gRememberCloseCheckbox);
    gSavedCloseAction = LoadSavedCloseAction();
    SendMessageW(gRememberCloseCheckbox, BM_SETCHECK, gSavedCloseAction ? BST_CHECKED : BST_UNCHECKED, 0);
    SetWindowTheme(gRememberCloseCheckbox, L"DarkMode_Explorer", nullptr);
    gSaveLogButton = CreateButton(overview, L"Save log", ID_SAVE_LOG);
    gShutdownButton = CreateButton(overview, L"Close properly", ID_SHUTDOWN);
    gHelpButton = CreateButton(overview, L"Help", ID_HELP);
    gLanguageLabel = CreateLabel(overview, L"Language");
    gLanguageCombo = CreateWindowExW(0, WC_COMBOBOXW, L"",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST | WS_VSCROLL,
        0, 0, 10, 10, overview,
        reinterpret_cast<HMENU>(static_cast<INT_PTR>(ID_LANGUAGE)), gInstance, nullptr);
    ApplyFont(gLanguageCombo);
    SetWindowTheme(gLanguageCombo, L"DarkMode_Explorer", nullptr);
    for (int index = 0; index < 5; ++index) {
        SendMessageW(gLanguageCombo, CB_ADDSTRING, 0,
                     reinterpret_cast<LPARAM>(UiLanguageNativeName(static_cast<UiLanguage>(index))));
    }
    SendMessageW(gLanguageCombo, CB_SETCURSEL, static_cast<WPARAM>(gLanguage), 0);
    gOldUsernameProc = reinterpret_cast<WNDPROC>(SetWindowLongPtrW(gUsernameEdit, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(CredentialEditProc)));
    gOldPasswordProc = reinterpret_cast<WNDPROC>(SetWindowLongPtrW(gPasswordEdit, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(CredentialEditProc)));

    HWND handshake = gPages[1].container;
    gHandshakeLabel = CreateLabel(handshake, L"Peer VLD0 key");
    gHandshakeEdit = CreateEdit(handshake, ID_HANDSHAKE_KEY);
    gHandshakeStartButton = CreateButton(handshake, L"Establish handshake", ID_HANDSHAKE_START);
    gHandshakeStatusButton = CreateButton(handshake, L"Check status", ID_HANDSHAKE_STATUS);
    gHandshakeResultLabel = CreateLabel(handshake, L"No handshake action requested yet.");

    HWND network = gPages[2].container;
    gWalkNormalLabel = CreateLabel(network, L"Normal mode");
    gWalkNormalMinHopsLabel = CreateLabel(network, L"Min hops");
    gWalkNormalMinHopsEdit = CreateEdit(network, ID_WALK_NORMAL_MIN_HOPS);
    gWalkNormalMaxHopsLabel = CreateLabel(network, L"Max hops");
    gWalkNormalMaxHopsEdit = CreateEdit(network, ID_WALK_NORMAL_MAX_HOPS);
    gWalkNormalMinSecsLabel = CreateLabel(network, L"Min sec");
    gWalkNormalMinSecsEdit = CreateEdit(network, ID_WALK_NORMAL_MIN_SECS);
    gWalkNormalTargetSecsLabel = CreateLabel(network, L"Target sec");
    gWalkNormalTargetSecsEdit = CreateEdit(network, ID_WALK_NORMAL_TARGET_SECS);
    gWalkNormalMaxSecsLabel = CreateLabel(network, L"Max sec");
    gWalkNormalMaxSecsEdit = CreateEdit(network, ID_WALK_NORMAL_MAX_SECS);

    gWalkMailLabel = CreateLabel(network, L"Mail mode");
    gWalkMailMinHopsLabel = CreateLabel(network, L"Min hops");
    gWalkMailMinHopsEdit = CreateEdit(network, ID_WALK_MAIL_MIN_HOPS);
    gWalkMailMaxHopsLabel = CreateLabel(network, L"Max hops");
    gWalkMailMaxHopsEdit = CreateEdit(network, ID_WALK_MAIL_MAX_HOPS);
    gWalkMailMinSecsLabel = CreateLabel(network, L"Min sec");
    gWalkMailMinSecsEdit = CreateEdit(network, ID_WALK_MAIL_MIN_SECS);
    gWalkMailTargetSecsLabel = CreateLabel(network, L"Target sec");
    gWalkMailTargetSecsEdit = CreateEdit(network, ID_WALK_MAIL_TARGET_SECS);
    gWalkMailMaxSecsLabel = CreateLabel(network, L"Max sec");
    gWalkMailMaxSecsEdit = CreateEdit(network, ID_WALK_MAIL_MAX_SECS);

    SetWindowTextW(gWalkNormalMinHopsEdit, L"5");
    SetWindowTextW(gWalkNormalMaxHopsEdit, L"100");
    SetWindowTextW(gWalkNormalMinSecsEdit, L"300");
    SetWindowTextW(gWalkNormalTargetSecsEdit, L"1800");
    SetWindowTextW(gWalkNormalMaxSecsEdit, L"7200");
    SetWindowTextW(gWalkMailMinHopsEdit, L"7");
    SetWindowTextW(gWalkMailMaxHopsEdit, L"135");
    SetWindowTextW(gWalkMailMinSecsEdit, L"120");
    SetWindowTextW(gWalkMailTargetSecsEdit, L"150");
    SetWindowTextW(gWalkMailMaxSecsEdit, L"600");

    gWalkMailModeCheckbox = CreateWindowExW(
        0, L"BUTTON", L"Use mail mode for automatic walks",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX,
        0, 0, 10, 10, network,
        reinterpret_cast<HMENU>(static_cast<INT_PTR>(ID_WALK_MAIL_MODE)),
        gInstance, nullptr);
    ApplyFont(gWalkMailModeCheckbox);
    SetWindowTheme(gWalkMailModeCheckbox, L"DarkMode_Explorer", nullptr);

    gWalkApplyButton = CreateButton(network, L"Apply settings", ID_WALK_APPLY);
    gWalkNormalStartButton = CreateButton(network, L"Start normal walk", ID_WALK_NORMAL_START);
    gWalkMailStartButton = CreateButton(network, L"Start mail walk", ID_WALK_MAIL_START);
    gWalkStatusButton = CreateButton(network, L"Walk status", ID_WALK_STATUS);
    gWalkStopButton = CreateButton(network, L"Stop walk", ID_WALK_STOP);
    gRouteStatusButton = CreateButton(network, L"Route status", ID_ROUTE_STATUS);
    gNodeListButton = CreateButton(network, L"Node list", ID_NODE_LIST);
    gDaemonStatusButton = CreateButton(network, L"Daemon status", ID_DAEMON_STATUS);

    HWND headers = gPages[3].container;
    gMainHeaderLabel = CreateLabel(headers, L"Subkey 0 - main/presence header");
    gMainHeaderEdit = CreateLog(headers);
    SetWindowTextW(gMainHeaderEdit, L"Waiting for the first header read...");
    gCopyMainHeaderButton = CreateButton(headers, L"Copy main header", ID_HEADERS_COPY_MAIN);
    gMailboxHeaderLabel = CreateLabel(headers, L"Subkey 2 - mailbox advertisement header");
    gMailboxHeaderEdit = CreateLog(headers);
    SetWindowTextW(gMailboxHeaderEdit, L"Waiting for the first header read...");
    gCopyMailboxHeaderButton = CreateButton(headers, L"Copy mailbox header", ID_HEADERS_COPY_MAILBOX);
    gRefreshHeadersButton = CreateButton(headers, L"Refresh headers", ID_HEADERS_REFRESH);

    HWND dht = gPages[4].container;
    gDhtNameLabel = CreateLabel(dht, L"Name");
    gDhtNameEdit = CreateEdit(dht, ID_DHT_NAME);
    gDhtGroupsLabel = CreateLabel(dht, L"Owner groups");
    gDhtGroupsEdit = CreateEdit(dht, ID_DHT_GROUPS);
    SetWindowTextW(gDhtGroupsEdit, L"250,1");
    gDhtCreateButton = CreateButton(dht, L"Create DHT", ID_DHT_CREATE);
    gDhtIndexLabel = CreateLabel(dht, L"Index");
    gDhtIndexEdit = CreateEdit(dht, ID_DHT_INDEX);
    SetWindowTextW(gDhtIndexEdit, L"0");
    gDhtSubkeyLabel = CreateLabel(dht, L"Subkey");
    gDhtSubkeyEdit = CreateEdit(dht, ID_DHT_SUBKEY);
    SetWindowTextW(gDhtSubkeyEdit, L"0");
    gDhtDataLabel = CreateLabel(dht, L"Value");
    gDhtDataEdit = CreateEdit(dht, ID_DHT_DATA);
    gDhtInspectButton = CreateButton(dht, L"Inspect", ID_DHT_INSPECT);
    gDhtWriteButton = CreateButton(dht, L"Write", ID_DHT_WRITE);
    gDhtReadButton = CreateButton(dht, L"Read", ID_DHT_READ);
    gDhtReadAllButton = CreateButton(dht, L"Read all", ID_DHT_READ_ALL);
    gDhtSaveButton = CreateButton(dht, L"Save DHTs", ID_DHT_SAVE);
    gDhtExternalKeyLabel = CreateLabel(dht, L"External VLD0 key");
    gDhtExternalKeyEdit = CreateEdit(dht, ID_DHT_EXTERNAL_KEY);
    gDhtLocationsLabel = CreateLabel(dht, L"Subkeys");
    gDhtLocationsEdit = CreateEdit(dht, ID_DHT_LOCATIONS);
    SetWindowTextW(gDhtLocationsEdit, L"0,1,10");
    gDhtExternalSelectedButton = CreateButton(dht, L"Read selected", ID_DHT_EXTERNAL_SELECTED);
    gDhtExternalAllButton = CreateButton(dht, L"Read all external", ID_DHT_EXTERNAL_ALL);

    HWND mailbox = gPages[5].container;
    gMailRecipientLabel = CreateLabel(mailbox, L"Recipient VLD0 key");
    gMailRecipientEdit = CreateEdit(mailbox, ID_MAIL_RECIPIENT);
    gMailAppLabel = CreateLabel(mailbox, L"Application");
    gMailAppEdit = CreateEdit(mailbox, ID_MAIL_APP);
    gMailPayloadLabel = CreateLabel(mailbox, L"Payload");
    gMailPayloadEdit = CreateEdit(mailbox, ID_MAIL_PAYLOAD);
    gMailSendButton = CreateButton(mailbox, L"Send mail", ID_MAIL_SEND);
    gMailStatusButton = CreateButton(mailbox, L"Status", ID_MAIL_STATUS);
    gMailListButton = CreateButton(mailbox, L"List inbox", ID_MAIL_LIST);
    gMailRetrieveButton = CreateButton(mailbox, L"Retrieve", ID_MAIL_RETRIEVE);
    gMailStatsButton = CreateButton(mailbox, L"Statistics", ID_MAIL_STATS);
    gMailFlushButton = CreateButton(mailbox, L"Flush", ID_MAIL_FLUSH);
    gMailRepairButton = CreateButton(mailbox, L"Repair", ID_MAIL_REPAIR);

    HWND applications = gPages[6].container;
    gAppIdLabel = CreateLabel(applications, L"Application id");
    gAppIdEdit = CreateEdit(applications, ID_APP_ID);
    gAppNameLabel = CreateLabel(applications, L"Display name");
    gAppNameEdit = CreateEdit(applications, ID_APP_NAME);
    gAppAddButton = CreateButton(applications, L"Add app", ID_APP_ADD);
    gAppListButton = CreateButton(applications, L"List apps", ID_APP_LIST);
    gAppRotateButton = CreateButton(applications, L"Rotate key", ID_APP_ROTATE);
    gAppPendingButton = CreateButton(applications, L"Pending requests", ID_APP_PENDING);
    gAppRequestLabel = CreateLabel(applications, L"Request id");
    gAppRequestEdit = CreateEdit(applications, ID_APP_REQUEST);
    gAppApproveButton = CreateButton(applications, L"Approve", ID_APP_APPROVE);
    gAppReasonLabel = CreateLabel(applications, L"Reject reason");
    gAppReasonEdit = CreateEdit(applications, ID_APP_REASON);
    gAppRejectButton = CreateButton(applications, L"Reject", ID_APP_REJECT);

    TabCtrl_SetCurSel(gTab, 0);
    ShowSelectedPage();
    ApplyLanguage();
}

void CleanupBackend() {
    RemoveTrayIcon();

    if (gProcessRunning && gProcess) {
        TerminateProcess(gProcess, 0xBADC0DE);
        WaitForSingleObject(gProcess, 1000);
    }
    gProcessRunning = false;

    {
        std::lock_guard<std::mutex> lock(gProcessMutex);
        if (gProcessStdin) {
            CloseHandle(gProcessStdin);
            gProcessStdin = nullptr;
        }
    }

    if (gReaderThread.joinable()) {
        gReaderThread.join();
    }
    if (gProcessStdout) {
        CloseHandle(gProcessStdout);
        gProcessStdout = nullptr;
    }
    if (gProcess) {
        CloseHandle(gProcess);
        gProcess = nullptr;
    }
}


LRESULT CALLBACK PageProcedure(HWND window, UINT message, WPARAM wParam, LPARAM lParam) {
    switch (message) {
    case WM_COMMAND:
    case WM_DRAWITEM:
        return SendMessageW(gMainWindow, message, wParam, lParam);
    case WM_CTLCOLORSTATIC:
    case WM_CTLCOLORBTN: {
        HDC dc = reinterpret_cast<HDC>(wParam);
        SetTextColor(dc, reinterpret_cast<HWND>(lParam) == gStatusLabel ? gStatusColor : kColorText);
        SetBkColor(dc, kColorPanel);
        return reinterpret_cast<LRESULT>(gPanelBrush);
    }
    case WM_CTLCOLOREDIT: {
        HDC dc = reinterpret_cast<HDC>(wParam);
        SetTextColor(dc, kColorText);
        SetBkColor(dc, kColorEdit);
        return reinterpret_cast<LRESULT>(gEditBrush);
    }
    case WM_ERASEBKGND: {
        RECT rect{};
        GetClientRect(window, &rect);
        FillRect(reinterpret_cast<HDC>(wParam), &rect, gPanelBrush);
        return 1;
    }
    default:
        return DefWindowProcW(window, message, wParam, lParam);
    }
}

LRESULT CALLBACK WindowProcedure(HWND window, UINT message, WPARAM wParam, LPARAM lParam) {
    switch (message) {
    case WM_CREATE: {
        BOOL darkMode = TRUE;
        DwmSetWindowAttribute(window, 20, &darkMode, sizeof(darkMode));
        DwmSetWindowAttribute(window, 19, &darkMode, sizeof(darkMode));
        CreateInterface(window);
        return 0;
    }
    case WM_SIZE: {
        if (wParam == SIZE_MINIMIZED &&
            SendMessageW(gMinimizeCheckbox, BM_GETCHECK, 0, 0) == BST_CHECKED) {
            AddTrayIcon();
            ShowWindow(window, SW_HIDE);
            return 0;
        }
        LayoutPages(LOWORD(lParam), HIWORD(lParam));
        return 0;
    }
    case WM_GETMINMAXINFO: {
        auto* info = reinterpret_cast<MINMAXINFO*>(lParam);
        RECT workArea{};
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &workArea, 0);
        info->ptMaxTrackSize.x = (workArea.right - workArea.left) / 2;
        info->ptMaxTrackSize.y = (workArea.bottom - workArea.top) / 2;
        info->ptMinTrackSize.x = std::min<LONG>(680, info->ptMaxTrackSize.x);
        info->ptMinTrackSize.y = std::min<LONG>(430, info->ptMaxTrackSize.y);
        return 0;
    }
    case WM_NOTIFY: {
        const auto* header = reinterpret_cast<NMHDR*>(lParam);
        if (header->idFrom == ID_TAB && header->code == TCN_SELCHANGE) {
            ShowSelectedPage();
            return 0;
        }
        break;
    }
    case WM_DRAWITEM:
        DrawOwnerControl(reinterpret_cast<DRAWITEMSTRUCT*>(lParam));
        return TRUE;
    case WM_CTLCOLORSTATIC: {
        HDC dc = reinterpret_cast<HDC>(wParam);
        SetTextColor(dc, reinterpret_cast<HWND>(lParam) == gStatusLabel ? gStatusColor : kColorText);
        SetBkColor(dc, kColorPanel);
        return reinterpret_cast<LRESULT>(gPanelBrush);
    }
    case WM_CTLCOLOREDIT: {
        HDC dc = reinterpret_cast<HDC>(wParam);
        SetTextColor(dc, kColorText);
        SetBkColor(dc, kColorEdit);
        return reinterpret_cast<LRESULT>(gEditBrush);
    }
    case WM_ERASEBKGND: {
        RECT rect{};
        GetClientRect(window, &rect);
        FillRect(reinterpret_cast<HDC>(wParam), &rect, gWindowBrush);
        return 1;
    }
    case WM_COMMAND: {
        const int id = LOWORD(wParam);
        switch (id) {
        case ID_LOGIN: SubmitCredentials(false); return 0;
        case ID_SIGNUP: SubmitCredentials(true); return 0;
        case ID_COPY_KEY: CopyMainKey(); return 0;
        case ID_SAVE_LOG: SaveSessionLog(); return 0;
        case ID_SHUTDOWN: BeginProperShutdown(); return 0;
        case ID_HELP: ShowHelp(); return 0;
        case ID_LANGUAGE:
            if (HIWORD(wParam) == CBN_SELCHANGE) {
                const LRESULT selected = SendMessageW(gLanguageCombo, CB_GETCURSEL, 0, 0);
                if (selected >= 0 && selected <= 4) {
                    gLanguage = static_cast<UiLanguage>(selected);
                    SaveUiLanguage();
                    ApplyLanguage();
                }
            }
            return 0;
        case ID_REMEMBER_CLOSE:
            if (SendMessageW(gRememberCloseCheckbox, BM_GETCHECK, 0, 0) != BST_CHECKED) SaveCloseAction(0);
            return 0;
        case ID_HANDSHAKE_START: StartHandshake(false); return 0;
        case ID_HANDSHAKE_STATUS: StartHandshake(true); return 0;
        case ID_WALK_APPLY:
        case ID_WALK_NORMAL_START:
        case ID_WALK_MAIL_START:
        case ID_WALK_STATUS:
        case ID_WALK_STOP:
        case ID_ROUTE_STATUS:
        case ID_NODE_LIST:
        case ID_DAEMON_STATUS:
            HandleNetworkAction(id); return 0;
        case ID_HEADERS_REFRESH:
        case ID_HEADERS_COPY_MAIN:
        case ID_HEADERS_COPY_MAILBOX:
            HandleHeaderAction(id); return 0;
        case ID_DHT_CREATE:
        case ID_DHT_INSPECT:
        case ID_DHT_WRITE:
        case ID_DHT_READ:
        case ID_DHT_READ_ALL:
        case ID_DHT_SAVE:
        case ID_DHT_EXTERNAL_SELECTED:
        case ID_DHT_EXTERNAL_ALL:
            HandleDhtAction(id); return 0;
        case ID_MAIL_SEND:
        case ID_MAIL_STATUS:
        case ID_MAIL_LIST:
        case ID_MAIL_RETRIEVE:
        case ID_MAIL_STATS:
        case ID_MAIL_FLUSH:
        case ID_MAIL_REPAIR:
            HandleMailboxAction(id); return 0;
        case ID_APP_ADD:
        case ID_APP_LIST:
        case ID_APP_ROTATE:
        case ID_APP_PENDING:
        case ID_APP_APPROVE:
        case ID_APP_REJECT:
            HandleApplicationAction(id); return 0;
        case ID_TRAY_OPEN: RestoreFromTray(); return 0;
        case ID_TRAY_PROPER: BeginProperShutdown(); return 0;
        case ID_TRAY_CRAZY: ForceClose(); return 0;
        default: break;
        }
        break;
    }
    case WM_APP_LOG_LINE: {
        auto* line = reinterpret_cast<std::wstring*>(lParam);
        if (line) {
            ProcessLogLine(*line);
            delete line;
        }
        return 0;
    }
    case WM_APP_BACKEND_EXIT: {
        gProcessRunning = false;
        const DWORD exitCode = static_cast<DWORD>(wParam);
        if (gClosingProperly) {
            DestroyWindow(window);
        } else {
            SetStatus(L"Backend stopped (exit code " + std::to_wstring(exitCode) + L")");
            EnableCredentialControls(true);
            gAuthenticated = false;
            gReady = false;
        }
        return 0;
    }
    case WM_APP_TRAY:
        if (LOWORD(lParam) == WM_LBUTTONDBLCLK) {
            RestoreFromTray();
        } else if (LOWORD(lParam) == WM_RBUTTONUP || LOWORD(lParam) == WM_CONTEXTMENU) {
            ShowTrayMenu();
        }
        return 0;
    case WM_CLOSE:
        PromptForClose();
        return 0;
    case WM_DESTROY:
        CleanupBackend();
        PostQuitMessage(0);
        return 0;
    default:
        break;
    }
    return DefWindowProcW(window, message, wParam, lParam);
}

} // namespace

int APIENTRY wWinMain(HINSTANCE instance, HINSTANCE, LPWSTR, int showCommand) {
    gInstance = instance;
    gLanguage = LoadUiLanguage();

    INITCOMMONCONTROLSEX controls{};
    controls.dwSize = sizeof(controls);
    controls.dwICC = ICC_TAB_CLASSES | ICC_STANDARD_CLASSES;
    InitCommonControlsEx(&controls);

    gWindowBrush = CreateSolidBrush(kColorWindow);
    gPanelBrush = CreateSolidBrush(kColorPanel);
    gEditBrush = CreateSolidBrush(kColorEdit);
    gUiFont = CreateFontW(-16, 0, 0, 0, FW_NORMAL, FALSE, FALSE, FALSE,
                         DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                         CLEARTYPE_QUALITY, DEFAULT_PITCH | FF_DONTCARE, L"Segoe UI");
    gTitleFont = CreateFontW(-23, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE,
                            DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                            CLEARTYPE_QUALITY, DEFAULT_PITCH | FF_DONTCARE, L"Segoe UI Semibold");

    WNDCLASSEXW pageClass{};
    pageClass.cbSize = sizeof(pageClass);
    pageClass.style = CS_HREDRAW | CS_VREDRAW;
    pageClass.lpfnWndProc = PageProcedure;
    pageClass.hInstance = instance;
    pageClass.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    pageClass.hbrBackground = gPanelBrush;
    pageClass.lpszClassName = kPageClass;
    if (!RegisterClassExW(&pageClass)) {
        return 1;
    }

    WNDCLASSEXW windowClass{};
    windowClass.cbSize = sizeof(windowClass);
    windowClass.style = CS_HREDRAW | CS_VREDRAW;
    windowClass.lpfnWndProc = WindowProcedure;
    windowClass.hInstance = instance;
    windowClass.hIcon = static_cast<HICON>(LoadImageW(instance, MAKEINTRESOURCEW(101), IMAGE_ICON, 0, 0, LR_DEFAULTSIZE));
    if (!windowClass.hIcon) windowClass.hIcon = LoadIconW(nullptr, IDI_APPLICATION);
    windowClass.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    windowClass.hbrBackground = gWindowBrush;
    windowClass.lpszClassName = kWindowClass;
    windowClass.hIconSm = windowClass.hIcon;
    if (!RegisterClassExW(&windowClass)) {
        return 1;
    }

    RECT workArea{};
    SystemParametersInfoW(SPI_GETWORKAREA, 0, &workArea, 0);
    const int availableWidth = static_cast<int>(workArea.right - workArea.left);
    const int availableHeight = static_cast<int>(workArea.bottom - workArea.top);
    const int width = std::min(920, availableWidth / 2);
    const int height = std::min(620, availableHeight / 2);
    const int x = workArea.left + ((workArea.right - workArea.left) - width) / 2;
    const int y = workArea.top + ((workArea.bottom - workArea.top) - height) / 2;

    gMainWindow = CreateWindowExW(0, kWindowClass, kWindowTitle,
                                  WS_OVERLAPPEDWINDOW & ~WS_MAXIMIZEBOX,
                                  x, y, width, height, nullptr, nullptr, instance, nullptr);
    if (!gMainWindow) {
        return 1;
    }

    ShowWindow(gMainWindow, showCommand);
    UpdateWindow(gMainWindow);

    MSG message{};
    while (GetMessageW(&message, nullptr, 0, 0) > 0) {
        if (!IsDialogMessageW(gMainWindow, &message)) {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    DeleteObject(gUiFont);
    DeleteObject(gTitleFont);
    DeleteObject(gWindowBrush);
    DeleteObject(gPanelBrush);
    DeleteObject(gEditBrush);
    return static_cast<int>(message.wParam);
}

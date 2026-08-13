#include <windows.h>
#include <commctrl.h>
#include <commdlg.h>
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
#include <utility>
#include <unordered_map>
#include <vector>

#include "ui_localization.h"

#pragma comment(lib, "Comctl32.lib")
#pragma comment(lib, "Comdlg32.lib")
#pragma comment(lib, "Dwmapi.lib")
#pragma comment(lib, "UxTheme.lib")

namespace {

constexpr wchar_t kWindowClass[] = L"VeilKnitDaemonGuiWindow";
constexpr wchar_t kPageClass[] = L"VeilKnitDaemonGuiPage";
constexpr wchar_t kWindowTitle[] = L"VeilKnit Daemon";

constexpr UINT WM_APP_LOG_LINE = WM_APP + 1;
constexpr UINT WM_APP_BACKEND_EXIT = WM_APP + 2;
constexpr UINT WM_APP_TRAY = WM_APP + 3;
constexpr UINT_PTR ID_SUMMARY_TIMER = 9001;

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
constexpr int ID_DISCORD = 126;
constexpr int ID_LANGUAGE = 127;
constexpr int ID_REMEMBER_CLOSE = 128;
constexpr int ID_RESTORE_BACKUP = 129;
constexpr int ID_HANDSHAKE_KEY = 130;
constexpr int ID_BACKUP_PASSPHRASE = 270;
constexpr int ID_BACKUP_LOCAL = 271;
constexpr int ID_BACKUP_UPLOAD = 272;
constexpr int ID_BACKUP_PATH = 273;
constexpr int ID_BACKUP_COPY_PATH = 274;
constexpr int ID_BACKUP_RECOVERY_CODE = 275;
constexpr int ID_BACKUP_COPY_CODE = 276;
constexpr int ID_BACKUP_DOWNLOAD = 277;
constexpr int ID_BACKUP_STATUS = 278;
constexpr int ID_BACKUP_WIPE = 279;
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
constexpr int ID_APP_NAME_DEFAULT = 190;
constexpr int ID_APP_NAME_ALIAS = 191;
constexpr int ID_APP_NAME_CLEAR = 192;
constexpr int ID_APP_NAME_LIST = 193;
constexpr int ID_APP_ADVANCED = 194;
constexpr int ID_APP_REQUEST_LIST = 195;
constexpr int ID_APP_FOUND_LIST = 196;
constexpr int ID_PROFILE_NAME = 260;
constexpr int ID_PROFILE_ID = 261;
constexpr int ID_PROFILE_CREATE = 262;
constexpr int ID_PROFILE_LIST = 263;
constexpr int ID_PROFILE_USE = 264;
constexpr int ID_PROFILE_RETIRE = 265;

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
HWND gRestoreBackupButton{};
HWND gBackupPassphraseLabel{};
HWND gBackupPassphraseEdit{};
HWND gBackupLocalButton{};
HWND gBackupUploadButton{};
HWND gBackupPathLabel{};
HWND gBackupPathEdit{};
HWND gBackupCopyPathButton{};
HWND gBackupRecoveryCodeLabel{};
HWND gBackupRecoveryCodeEdit{};
HWND gBackupCopyCodeButton{};
HWND gBackupDownloadButton{};
HWND gBackupStatusButton{};
HWND gBackupWipeButton{};
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
HWND gDiscordButton{};
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

HWND gSummaryTopologyGroup{};
HWND gSummaryTopologyText{};
HWND gSummaryPresenceGroup{};
HWND gSummaryPresenceText{};
HWND gSummaryHeadersGroup{};
HWND gSummaryHeadersText{};
HWND gSummaryActivityGroup{};
HWND gSummaryActivityText{};

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
HWND gAppRequestList{};
HWND gAppFoundLabel{};
HWND gAppFoundList{};
HWND gAppRequestLabel{};
HWND gAppRequestEdit{};
HWND gAppApproveButton{};
HWND gAppReasonLabel{};
HWND gAppReasonEdit{};
HWND gAppRejectButton{};
HWND gAppVisibleDefaultButton{};
HWND gAppVisibleAliasButton{};
HWND gAppVisibleClearButton{};
HWND gAppVisibleListButton{};
HWND gAppAdvancedButton{};
bool gAppAdvancedExpanded = false;
HWND gProfileNameLabel{};
HWND gProfileNameEdit{};
HWND gProfileIdLabel{};
HWND gProfileIdEdit{};
HWND gProfileCreateButton{};
HWND gProfileListButton{};
HWND gProfileUseButton{};
HWND gProfileRetireButton{};

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
bool CopyTextToClipboard(const std::wstring& value);
void ShowSelectedPage();

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

void SetListColumnText(HWND list, int column, const wchar_t* english) {
    if (!list) return;
    LVCOLUMNW item{};
    item.mask = LVCF_TEXT;
    item.pszText = const_cast<wchar_t*>(T(english));
    ListView_SetColumn(list, column, &item);
}

void RefreshWindowPaint() {
    if (!gMainWindow) return;
    if (gLanguageCombo) {
        InvalidateRect(gLanguageCombo, nullptr, TRUE);
        RedrawWindow(gLanguageCombo, nullptr, nullptr,
            RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN | RDW_UPDATENOW);
    }
    RedrawWindow(gMainWindow, nullptr, nullptr,
        RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN | RDW_UPDATENOW);
}

void ApplyLanguage() {
    if (!gMainWindow) return;
    SetWindowTextW(gMainWindow, T(L"VeilKnit Daemon"));
    SetUiText(gHeaderTitle, L"VeilKnit Daemon");
    SetUiText(gUsernameLabel, L"Username");
    SetUiText(gPasswordLabel, L"Password");
    SetUiText(gLanguageLabel, L"🌐 Language");
    SetUiText(gLoginButton, L"Login");
    SetUiText(gSignupButton, L"Sign up");
    SetUiText(gRestoreBackupButton, L"Restore backup");
    SetUiText(gBackupPassphraseLabel, L"Backup passphrase");
    SetUiText(gBackupLocalButton, L"Create local backup");
    SetUiText(gBackupUploadButton, L"Create backup and upload recovery copy");
    SetUiText(gBackupPathLabel, L"Backup file");
    SetUiText(gBackupCopyPathButton, L"Copy backup path");
    SetUiText(gBackupRecoveryCodeLabel, L"Recovery code");
    SetUiText(gBackupCopyCodeButton, L"Copy recovery code");
    SetUiText(gBackupDownloadButton, L"Download recovery backup");
    SetUiText(gBackupStatusButton, L"Recovery status");
    SetUiText(gBackupWipeButton, L"Wipe network recovery");
    SetUiText(gMainKeyLabel, L"Main key");
    SetUiText(gCopyKeyButton, L"Copy key");
    SetUiText(gMinimizeCheckbox, L"Minimize to notification area");
    SetUiText(gRememberCloseCheckbox, L"Always use saved X-button action");
    SetUiText(gSaveLogButton, L"Save log");
    SetUiText(gShutdownButton, L"Close properly");
    SetUiText(gHelpButton, L"Help");
    SetUiText(gDiscordButton, L"Discord");
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
    SetUiText(gSummaryTopologyGroup, L"Topology");
    SetUiText(gSummaryPresenceGroup, L"Presence");
    SetUiText(gSummaryHeadersGroup, L"Header cache");
    SetUiText(gSummaryActivityGroup, L"Activity");
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
    SetUiText(gAppPendingButton, L"Refresh requests");
    SetUiText(gAppFoundLabel, L"Observed applications");
    SetUiText(gAppRequestLabel, L"Request id");
    SetUiText(gAppApproveButton, L"Allow checked");
    SetUiText(gAppReasonLabel, L"Refusal reason");
    SetUiText(gAppRejectButton, L"Refuse checked");
    SetListColumnText(gAppRequestList, 0, L"Application");
    SetListColumnText(gAppRequestList, 1, L"Display name");
    SetListColumnText(gAppRequestList, 2, L"Request");
    SetListColumnText(gAppFoundList, 0, L"Application");
    SetListColumnText(gAppFoundList, 1, L"Verified headers");
    SetListColumnText(gAppFoundList, 2, L"Discovery cache");
    SetListColumnText(gAppFoundList, 3, L"Recent");
    SetListColumnText(gAppFoundList, 4, L"Archive");
    SetUiText(gAppVisibleDefaultButton, L"Set default visible name");
    SetUiText(gAppVisibleAliasButton, L"Set app alias");
    SetUiText(gAppVisibleClearButton, L"Clear alias");
    SetUiText(gAppVisibleListButton, L"List aliases");
    SetUiText(gAppAdvancedButton, gAppAdvancedExpanded ? L"Advanced application management ▾" : L"Advanced application management ▸");
    SetUiText(gProfileNameLabel, L"New profile name");
    SetUiText(gProfileIdLabel, L"Profile id");
    SetUiText(gProfileCreateButton, L"Create profile");
    SetUiText(gProfileListButton, L"List profiles");
    SetUiText(gProfileUseButton, L"Use after restart");
    SetUiText(gProfileRetireButton, L"Retire profile");

    const wchar_t* tabs[] = {L"Applications", L"Backup", L"Overview", L"Handshake", L"Network", L"Headers", L"DHT", L"Mailbox", L"All logs"};
    for (int index = 0; index < 9; ++index) {
        TCITEMW item{};
        item.mask = TCIF_TEXT;
        item.pszText = const_cast<wchar_t*>(T(tabs[index]));
        TabCtrl_SetItem(gTab, index, &item);
    }
    RefreshWindowPaint();
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

std::wstring DecodeHexUtf8(const std::wstring& value) {
    if (value.size() % 2 != 0) return {};
    auto nibble = [](wchar_t ch) -> int {
        if (ch >= L'0' && ch <= L'9') return ch - L'0';
        if (ch >= L'a' && ch <= L'f') return ch - L'a' + 10;
        if (ch >= L'A' && ch <= L'F') return ch - L'A' + 10;
        return -1;
    };
    std::string bytes;
    bytes.reserve(value.size() / 2);
    for (size_t index = 0; index < value.size(); index += 2) {
        const int high = nibble(value[index]);
        const int low = nibble(value[index + 1]);
        if (high < 0 || low < 0) return {};
        bytes.push_back(static_cast<char>((high << 4) | low));
    }
    return Utf8ToWide(bytes);
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
          L"3. The newest request for each application appears in the request list. "
          L"Check the applications you want and choose Allow checked or Refuse checked.\n\n"
          L"The Network tab contains separate adaptive settings and manual start buttons "
          L"for normal and mail walks. The Headers tab reads and displays your published "
          L"subkey 0 presence header and subkey 2 mailbox advertisement.\n\n"
          L"Green status means ready, orange means starting or reconnecting, and red means "
          L"stopped or failed.\n\n"
          L"Community and support: https://discord.gg/yy5SMTuZY\n"
          L"Discord is an external service and is not required to use VeilKnit."),
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

HWND CreateGroup(HWND parent, const wchar_t* text) {
    HWND control = CreateWindowExW(0, L"BUTTON", text,
                                   WS_CHILD | WS_VISIBLE | BS_GROUPBOX,
                                   0, 0, 10, 10, parent, nullptr, gInstance, nullptr);
    ApplyFont(control);
    SetWindowTheme(control, L"DarkMode_Explorer", nullptr);
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
    EnableWindow(gRestoreBackupButton, enabled);
}

void EnableBackupControls(bool enabled) {
    EnableWindow(gBackupLocalButton, enabled);
    EnableWindow(gBackupUploadButton, enabled);
    EnableWindow(gBackupDownloadButton, enabled);
    EnableWindow(gBackupStatusButton, enabled);
    EnableWindow(gBackupWipeButton, enabled);
}

unsigned long long SummaryNumber(
    const std::unordered_map<std::wstring, std::wstring>& fields,
    const wchar_t* name) {
    auto found = fields.find(name);
    if (found == fields.end()) return 0;
    try { return std::stoull(found->second); } catch (...) { return 0; }
}

void ApplyGuiSummary(const std::wstring& marker) {
    std::unordered_map<std::wstring, std::wstring> fields;
    std::wstringstream stream(marker);
    std::wstring part;
    while (std::getline(stream, part, L';')) {
        const size_t equals = part.find(L'=');
        if (equals == std::wstring::npos || equals == 0) continue;
        fields[part.substr(0, equals)] = part.substr(equals + 1);
    }

    const auto n = [&](const wchar_t* key) { return SummaryNumber(fields, key); };
    std::wstringstream topology;
    topology << T(L"Verified") << L": " << n(L"verified") << L"\r\n"
             << T(L"Candidates") << L": " << n(L"candidates") << L"\r\n"
             << T(L"Authenticated") << L": " << n(L"authenticated");
    SetWindowTextW(gSummaryTopologyText, topology.str().c_str());

    std::wstringstream presence;
    presence << T(L"Online") << L": " << n(L"online") << L"\r\n"
             << T(L"Offline") << L": " << n(L"offline") << L"\r\n"
             << T(L"Stale claim") << L": " << n(L"stale") << L"\r\n"
             << T(L"Needs refresh") << L": " << n(L"refresh") << L"\r\n"
             << T(L"Unknown") << L": " << n(L"unknown");
    SetWindowTextW(gSummaryPresenceText, presence.str().c_str());

    std::wstringstream headers;
    headers << T(L"Presence OK") << L": " << n(L"presence_ok") << L"\r\n"
            << T(L"Read failed") << L": " << n(L"presence_failed") << L"\r\n"
            << T(L"Not checked") << L": " << n(L"presence_unread") << L"\r\n"
            << T(L"Active app info") << L": " << n(L"app_headers") << L"\r\n"
            << T(L"Mailbox capable") << L": " << n(L"mailbox_capable");
    SetWindowTextW(gSummaryHeadersText, headers.str().c_str());

    std::wstring walk = L"idle";
    if (auto found = fields.find(L"walk_state"); found != fields.end()) walk = found->second;
    std::wstringstream activity;
    activity << T(L"Walk") << L": " << walk;
    if (n(L"walk_total") > 0) activity << L" " << n(L"walk_done") << L"/" << n(L"walk_total");
    activity << L"\r\n" << T(L"New / updated") << L": " << n(L"walk_new") << L" / " << n(L"walk_updated")
             << L"\r\n" << T(L"Reach / fail") << L": " << n(L"walk_reachable") << L" / " << n(L"walk_unreachable")
             << L"\r\n" << T(L"App searches") << L": " << n(L"app_searches")
             << L"\r\n" << T(L"Root lookups") << L": " << n(L"root_lookups");
    SetWindowTextW(gSummaryActivityText, activity.str().c_str());
}

std::unordered_map<std::wstring, std::wstring> ParseGuiFields(const std::wstring& marker) {
    std::unordered_map<std::wstring, std::wstring> fields;
    std::wstringstream stream(marker);
    std::wstring part;
    while (std::getline(stream, part, L';')) {
        const size_t equals = part.find(L'=');
        if (equals == std::wstring::npos || equals == 0) continue;
        fields[part.substr(0, equals)] = part.substr(equals + 1);
    }
    return fields;
}

void AppendPendingRequestMarker(const std::wstring& marker) {
    if (!gAppRequestList) return;
    const auto fields = ParseGuiFields(marker);
    const auto requestIt = fields.find(L"request_id");
    const auto appIt = fields.find(L"app_hex");
    const auto nameIt = fields.find(L"name_hex");
    if (requestIt == fields.end() || appIt == fields.end() || nameIt == fields.end()) return;
    unsigned long long requestId = 0;
    try { requestId = std::stoull(requestIt->second); } catch (...) { return; }
    std::wstring app = DecodeHexUtf8(appIt->second);
    std::wstring name = DecodeHexUtf8(nameIt->second);
    if (app.empty()) return;

    LVITEMW item{};
    item.mask = LVIF_TEXT | LVIF_PARAM;
    item.iItem = ListView_GetItemCount(gAppRequestList);
    item.pszText = app.data();
    item.lParam = static_cast<LPARAM>(requestId);
    const int row = ListView_InsertItem(gAppRequestList, &item);
    if (row < 0) return;
    ListView_SetItemText(gAppRequestList, row, 1, name.data());
    std::wstring requestText = L"#" + std::to_wstring(requestId);
    ListView_SetItemText(gAppRequestList, row, 2, requestText.data());
}

void AppendFoundAppMarker(const std::wstring& marker) {
    if (!gAppFoundList) return;
    const auto fields = ParseGuiFields(marker);
    const auto appIt = fields.find(L"app_hex");
    if (appIt == fields.end()) return;
    std::wstring app = DecodeHexUtf8(appIt->second);
    if (app.empty()) return;
    const auto value = [&](const wchar_t* key) -> std::wstring {
        auto found = fields.find(key);
        return found == fields.end() ? L"0" : found->second;
    };
    LVITEMW item{};
    item.mask = LVIF_TEXT;
    item.iItem = ListView_GetItemCount(gAppFoundList);
    item.pszText = app.data();
    const int row = ListView_InsertItem(gAppFoundList, &item);
    if (row < 0) return;
    for (int column = 1; column <= 4; ++column) {
        const wchar_t* key = column == 1 ? L"observed" : column == 2 ? L"cached" : column == 3 ? L"recent" : L"archive";
        std::wstring text = value(key);
        ListView_SetItemText(gAppFoundList, row, column, text.data());
    }
}

std::vector<unsigned long long> CheckedRequestIds() {
    std::vector<unsigned long long> ids;
    if (!gAppRequestList) return ids;
    const int count = ListView_GetItemCount(gAppRequestList);
    for (int row = 0; row < count; ++row) {
        if (!ListView_GetCheckState(gAppRequestList, row)) continue;
        LVITEMW item{};
        item.mask = LVIF_PARAM;
        item.iItem = row;
        if (ListView_GetItem(gAppRequestList, &item) && item.lParam > 0) {
            ids.push_back(static_cast<unsigned long long>(item.lParam));
        }
    }
    return ids;
}

void ProcessLogLine(const std::wstring& line) {
    if (gPages.size() < 9) {
        return;
    }

    const std::wstring lower = ToLower(line);
    if (line.find(L"GUI_APP_REQUESTS_BEGIN") != std::wstring::npos) {
        if (gAppRequestList) ListView_DeleteAllItems(gAppRequestList);
        return;
    }
    const std::wstring requestMarker = L"GUI_APP_REQUEST=";
    if (const size_t position = line.find(requestMarker); position != std::wstring::npos) {
        AppendPendingRequestMarker(line.substr(position + requestMarker.size()));
        return;
    }
    if (line.find(L"GUI_APP_REQUESTS_END") != std::wstring::npos) return;
    if (line.find(L"GUI_APPS_BEGIN") != std::wstring::npos) {
        if (gAppFoundList) ListView_DeleteAllItems(gAppFoundList);
        return;
    }
    const std::wstring appMarker = L"GUI_APP=";
    if (const size_t position = line.find(appMarker); position != std::wstring::npos) {
        AppendFoundAppMarker(line.substr(position + appMarker.size()));
        return;
    }
    if (line.find(L"GUI_APPS_END") != std::wstring::npos) return;

    const std::wstring summaryMarker = L"GUI_SUMMARY=";
    const size_t summaryPosition = line.find(summaryMarker);
    if (summaryPosition != std::wstring::npos) {
        ApplyGuiSummary(line.substr(summaryPosition + summaryMarker.size()));
        return;
    }
    AppendToLog(gPages[7].log, line); // All logs

    const std::wstring recoveryCodeMarker = L"RECOVERY CODE (store this privately):";
    if (const size_t recoveryPosition = line.find(recoveryCodeMarker); recoveryPosition != std::wstring::npos) {
        std::wstring code = line.substr(recoveryPosition + recoveryCodeMarker.size());
        while (!code.empty() && iswspace(code.front())) code.erase(code.begin());
        if (gBackupRecoveryCodeEdit) SetWindowTextW(gBackupRecoveryCodeEdit, code.c_str());
    }
    if (ContainsAny(lower, {L"backup", L"recovery"})) {
        AppendToLog(gPages[8].log, line);
    }

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
        TabCtrl_SetCurSel(gTab, 0);
        ShowSelectedPage();
        SetConnectionStatus(T(L"Authenticated; starting network services..."), kColorWarning);
        SetWindowTextW(gPasswordEdit, L"");
    }

    if (line.find(L"[gui] READY") != std::wstring::npos) {
        gReady = true;
        EnableCredentialControls(false);
        EnableBackupControls(true);
        SetConnectionStatus(T(L"Running"), kColorSuccess);
        SendBackendLine(L"walk-settings");
        SendBackendLine(L"headers");
        SendBackendLine(L"summary");
        SendBackendLine(L"app-pending");
    }

    if (ContainsAny(lower, {L"no account with that username", L"wrong password",
                            L"username is already taken", L"usernames may only contain"})) {
        gAuthenticated = false;
        TabCtrl_SetCurSel(gTab, 2);
        ShowSelectedPage();
        EnableCredentialControls(true);
        SetConnectionStatus(T(L"Authentication failed; correct the details and try again."), kColorRed);
        SetFocus(gPasswordEdit);
    }
    if (ContainsAny(lower, {L"restored account", L"could not restore backup"})) {
        gAuthenticated = false;
        EnableCredentialControls(true);
        SetConnectionStatus(
            lower.find(L"restored account") != std::wstring::npos
                ? T(L"Backup restored; log in with the account's original password.")
                : T(L"Backup restore failed; check the file and backup passphrase."),
            lower.find(L"restored account") != std::wstring::npos ? kColorSuccess : kColorRed);
        SetFocus(gUsernameEdit);
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

std::wstring SelectBackupFile() {
    wchar_t fileName[32768]{};
    OPENFILENAMEW dialog{};
    dialog.lStructSize = sizeof(dialog);
    dialog.hwndOwner = gMainWindow;
    dialog.lpstrFilter = L"VeilKnit backups (*.veilknit-backup)\0*.veilknit-backup\0All files (*.*)\0*.*\0\0";
    dialog.lpstrFile = fileName;
    dialog.nMaxFile = ARRAYSIZE(fileName);
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    dialog.lpstrDefExt = L"veilknit-backup";
    return GetOpenFileNameW(&dialog) ? std::wstring(fileName) : std::wstring();
}

void RestoreLocalBackup() {
    const std::wstring path = SelectBackupFile();
    if (path.empty()) return;
    const std::wstring passphrase = WindowText(gPasswordEdit);
    if (passphrase.empty() || passphrase.find_first_of(L"\r\n") != std::wstring::npos) {
        MessageBoxW(
            gMainWindow,
            T(L"Enter the backup passphrase in the Password field, then press Restore backup."),
            T(L"VeilKnit Daemon"), MB_OK | MB_ICONINFORMATION);
        SetFocus(gPasswordEdit);
        return;
    }
    if (!StartBackendProcess()) return;

    EnableCredentialControls(false);
    SetConnectionStatus(T(L"Restoring encrypted backup..."), kColorWarning);
    const std::wstring payload = L"r\n" + path + L"\n" + passphrase + L"\n";
    if (!WriteBackendBytes(WideToUtf8(payload))) {
        EnableCredentialControls(true);
        SetConnectionStatus(T(L"Could not send the restore request to the backend."), kColorRed);
    }
    SetWindowTextW(gPasswordEdit, L"");
}

std::wstring SelectBackupOutputFile(const wchar_t* title, const wchar_t* suggestedName) {
    wchar_t fileName[MAX_PATH * 4]{};
    if (suggestedName) {
        wcsncpy_s(fileName, suggestedName, _TRUNCATE);
    }
    OPENFILENAMEW dialog{};
    dialog.lStructSize = sizeof(dialog);
    dialog.hwndOwner = gMainWindow;
    dialog.lpstrTitle = title;
    dialog.lpstrFilter = L"VeilKnit backups (*.veilknit-backup)\0*.veilknit-backup\0All files (*.*)\0*.*\0\0";
    dialog.lpstrFile = fileName;
    dialog.nMaxFile = ARRAYSIZE(fileName);
    dialog.Flags = OFN_PATHMUSTEXIST | OFN_OVERWRITEPROMPT | OFN_NOCHANGEDIR;
    dialog.lpstrDefExt = L"veilknit-backup";
    return GetSaveFileNameW(&dialog) ? std::wstring(fileName) : std::wstring();
}

bool ValidBackupPassphrase(const std::wstring& value) {
    return value.size() >= 8 && IsSingleLine(value);
}

void QueueBackupWithOptionalRecovery(bool uploadRecovery) {
    if (!RequireReady()) return;
    const std::wstring passphrase = WindowText(gBackupPassphraseEdit);
    if (!ValidBackupPassphrase(passphrase)) {
        MessageBoxW(gMainWindow,
                    T(L"Use a backup passphrase of at least 8 characters, without line breaks."),
                    T(L"VeilKnit Daemon"), MB_OK | MB_ICONWARNING);
        SetFocus(gBackupPassphraseEdit);
        return;
    }

    const std::wstring path = SelectBackupOutputFile(
        uploadRecovery ? T(L"Create backup and upload recovery copy") : T(L"Create local backup"),
        uploadRecovery ? L"veilknit-network-source.veilknit-backup" : L"veilknit-local.veilknit-backup");
    if (path.empty()) return;

    std::vector<std::wstring> commands = {
        L"backup-local " + path,
        passphrase,
        passphrase,
    };
    if (uploadRecovery) {
        commands.push_back(L"recovery-upload " + path);
    }
    if (!SendBackendLines(commands)) {
        SetConnectionStatus(T(L"Could not send the backup request to the backend."), kColorRed);
        return;
    }
    SetWindowTextW(gBackupPathEdit, path.c_str());
    SetWindowTextW(gBackupPassphraseEdit, L"");
    SetConnectionStatus(uploadRecovery
                            ? T(L"Backup creation and recovery upload queued...")
                            : T(L"Encrypted backup creation queued..."),
                        kColorWarning);
}

void DownloadRecoveryBackup() {
    if (!RequireReady()) return;
    std::wstring code = WindowText(gBackupRecoveryCodeEdit);
    while (!code.empty() && iswspace(code.front())) code.erase(code.begin());
    while (!code.empty() && iswspace(code.back())) code.pop_back();
    if (code.empty() || code.rfind(L"VKR1|", 0) != 0 || !IsSingleLine(code)) {
        MessageBoxW(gMainWindow, T(L"Enter a valid VKR1 recovery code first."),
                    T(L"VeilKnit Daemon"), MB_OK | MB_ICONWARNING);
        SetFocus(gBackupRecoveryCodeEdit);
        return;
    }

    const std::wstring path = SelectBackupOutputFile(
        T(L"Download recovery backup"), L"veilknit-recovered.veilknit-backup");
    if (path.empty()) return;
    if (!SendBackendLine(L"recovery-download " + code + L" " + path)) {
        SetConnectionStatus(T(L"Could not send the recovery download request."), kColorRed);
        return;
    }
    SetWindowTextW(gBackupPathEdit, path.c_str());
    SetConnectionStatus(T(L"Recovery backup download queued..."), kColorWarning);
}

void HandleBackupAction(int id) {
    switch (id) {
    case ID_BACKUP_LOCAL:
        QueueBackupWithOptionalRecovery(false);
        break;
    case ID_BACKUP_UPLOAD:
        QueueBackupWithOptionalRecovery(true);
        break;
    case ID_BACKUP_COPY_PATH: {
        const std::wstring value = WindowText(gBackupPathEdit);
        if (value.empty()) {
            MessageBoxW(gMainWindow, T(L"There is no backup path to copy yet."),
                        T(L"VeilKnit Daemon"), MB_OK | MB_ICONINFORMATION);
        } else {
            CopyTextToClipboard(value);
        }
        break;
    }
    case ID_BACKUP_COPY_CODE: {
        const std::wstring value = WindowText(gBackupRecoveryCodeEdit);
        if (value.empty()) {
            MessageBoxW(gMainWindow, T(L"There is no recovery code to copy yet."),
                        T(L"VeilKnit Daemon"), MB_OK | MB_ICONINFORMATION);
        } else {
            CopyTextToClipboard(value);
        }
        break;
    }
    case ID_BACKUP_DOWNLOAD:
        DownloadRecoveryBackup();
        break;
    case ID_BACKUP_STATUS:
        if (RequireReady()) SendBackendLine(L"recovery-status");
        break;
    case ID_BACKUP_WIPE:
        if (!RequireReady()) return;
        if (MessageBoxW(gMainWindow,
                        T(L"Wipe the current network recovery record? This replaces its data with tombstones."),
                        T(L"VeilKnit Daemon"), MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2) == IDYES) {
            SendBackendLines({L"recovery-wipe", L"WIPE"});
        }
        break;
    default:
        break;
    }
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
        const auto requests = CheckedRequestIds();
        if (requests.empty()) {
            MessageBoxW(gMainWindow, T(L"Check one or more application requests first."),
                        kWindowTitle, MB_OK | MB_ICONINFORMATION);
            break;
        }
        std::vector<std::wstring> commands;
        for (auto request : requests) commands.push_back(L"app-approve " + std::to_wstring(request));
        commands.push_back(L"app-pending");
        SendBackendLines(commands);
        break;
    }
    case ID_APP_REJECT: {
        const auto requests = CheckedRequestIds();
        if (requests.empty()) {
            MessageBoxW(gMainWindow, T(L"Check one or more application requests first."),
                        kWindowTitle, MB_OK | MB_ICONINFORMATION);
            break;
        }
        std::wstring reason = WindowText(gAppReasonEdit);
        if (reason.empty()) reason = L"rejected by the local user";
        if (!IsSingleLine(reason)) break;
        std::vector<std::wstring> commands;
        for (auto request : requests) {
            commands.push_back(L"app-reject " + std::to_wstring(request) + L" " + reason);
        }
        commands.push_back(L"app-pending");
        SendBackendLines(commands);
        break;
    }
    case ID_APP_NAME_DEFAULT: {
        const std::wstring name = WindowText(gAppNameEdit);
        if (!name.empty() && IsSingleLine(name)) SendBackendLine(L"app-name default " + name);
        break;
    }
    case ID_APP_NAME_ALIAS: {
        const std::wstring name = WindowText(gAppNameEdit);
        if (!appId.empty() && !name.empty() && IsSingleLine(appId) && IsSingleLine(name)) {
            SendBackendLine(L"app-name set " + appId + L" " + name);
        }
        break;
    }
    case ID_APP_NAME_CLEAR:
        if (!appId.empty() && IsSingleLine(appId)) SendBackendLine(L"app-name clear " + appId);
        break;
    case ID_APP_NAME_LIST: SendBackendLine(L"app-name"); break;
    case ID_PROFILE_CREATE: {
        const std::wstring name = WindowText(gProfileNameEdit);
        if (!name.empty() && IsSingleLine(name)) SendBackendLine(L"profile-create " + name);
        break;
    }
    case ID_PROFILE_LIST: SendBackendLine(L"profile-list"); break;
    case ID_PROFILE_USE: {
        const std::wstring profileId = WindowText(gProfileIdEdit);
        if (!profileId.empty() && IsSingleLine(profileId)) SendBackendLine(L"profile-use " + profileId);
        break;
    }
    case ID_PROFILE_RETIRE: {
        const std::wstring profileId = WindowText(gProfileIdEdit);
        if (!profileId.empty() && IsSingleLine(profileId)) SendBackendLine(L"profile-retire " + profileId);
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

    if (gPages.size() < 9) {
        return;
    }

    const int padding = 14;
    const int buttonWidth = 112;
    const int rowHeight = 28;

    MoveWindow(gUsernameLabel, padding, 17, 76, 22, TRUE);
    MoveWindow(gUsernameEdit, 86, 12, 180, rowHeight, TRUE);
    MoveWindow(gLoginButton, 280, 11, 82, 30, TRUE);
    MoveWindow(gSignupButton, 370, 11, 86, 30, TRUE);
    MoveWindow(gRestoreBackupButton, 464, 11, 120, 30, TRUE);
    MoveWindow(gHelpButton, 592, 11, 72, 30, TRUE);
    MoveWindow(gPasswordLabel, padding, 53, 74, 22, TRUE);
    MoveWindow(gPasswordEdit, 86, 48, 180, rowHeight, TRUE);
    MoveWindow(gLanguageLabel, 280, 53, 98, 22, TRUE);
    MoveWindow(gLanguageCombo, 382, 48, 164, 180, TRUE);
    MoveWindow(gDiscordButton, 552, 48, 88, 30, TRUE);

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

    // Structured network summary. These values are derived from daemon state,
    // not parsed from the human-readable log.
    const int summaryGap = 6;
    const int summaryHeight = 108;
    const int availableSummaryWidth = pageWidth - padding * 2 - summaryGap * 3;
    const int summaryWidth = std::max(100, availableSummaryWidth / 4);
    const int summaryWidths[4] = {
        summaryWidth, summaryWidth, summaryWidth,
        std::max(100, availableSummaryWidth - summaryWidth * 3)
    };
    HWND summaryGroups[4] = {
        gSummaryTopologyGroup, gSummaryPresenceGroup, gSummaryHeadersGroup, gSummaryActivityGroup
    };
    HWND summaryTexts[4] = {
        gSummaryTopologyText, gSummaryPresenceText, gSummaryHeadersText, gSummaryActivityText
    };
    int summaryX = padding;
    for (int index = 0; index < 4; ++index) {
        MoveWindow(summaryGroups[index], summaryX, 8, summaryWidths[index], summaryHeight, TRUE);
        MoveWindow(summaryTexts[index], summaryX + 8, 29, summaryWidths[index] - 16,
                   summaryHeight - 27, TRUE);
        summaryX += summaryWidths[index] + summaryGap;
    }
    const int networkOffset = 8 + summaryHeight + 6;

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

    layoutHopRow(networkOffset + 12, gWalkNormalLabel, gWalkNormalMinHopsLabel, gWalkNormalMinHopsEdit,
                 gWalkNormalMaxHopsLabel, gWalkNormalMaxHopsEdit);
    layoutIntervalRow(networkOffset + 46, gWalkNormalMinSecsLabel, gWalkNormalMinSecsEdit,
                      gWalkNormalTargetSecsLabel, gWalkNormalTargetSecsEdit,
                      gWalkNormalMaxSecsLabel, gWalkNormalMaxSecsEdit);

    layoutHopRow(networkOffset + 82, gWalkMailLabel, gWalkMailMinHopsLabel, gWalkMailMinHopsEdit,
                 gWalkMailMaxHopsLabel, gWalkMailMaxHopsEdit);
    layoutIntervalRow(networkOffset + 116, gWalkMailMinSecsLabel, gWalkMailMinSecsEdit,
                      gWalkMailTargetSecsLabel, gWalkMailTargetSecsEdit,
                      gWalkMailMaxSecsLabel, gWalkMailMaxSecsEdit);

    MoveWindow(gWalkMailModeCheckbox, padding, networkOffset + 151, 238, 24, TRUE);
    const int actionGap = 8;
    const int threeButtonWidth = std::max(96, (pageWidth - padding * 2 - actionGap * 2) / 3);
    int actionX = padding;
    MoveWindow(gWalkApplyButton, actionX, networkOffset + 180, threeButtonWidth, 30, TRUE);
    actionX += threeButtonWidth + actionGap;
    MoveWindow(gWalkNormalStartButton, actionX, networkOffset + 180, threeButtonWidth, 30, TRUE);
    actionX += threeButtonWidth + actionGap;
    MoveWindow(gWalkMailStartButton, actionX, networkOffset + 180,
               std::max(96, pageWidth - padding - actionX), 30, TRUE);

    const int fiveButtonWidth = std::max(76, (pageWidth - padding * 2 - actionGap * 4) / 5);
    actionX = padding;
    MoveWindow(gWalkStatusButton, actionX, networkOffset + 218, fiveButtonWidth, 30, TRUE);
    actionX += fiveButtonWidth + actionGap;
    MoveWindow(gWalkStopButton, actionX, networkOffset + 218, fiveButtonWidth, 30, TRUE);
    actionX += fiveButtonWidth + actionGap;
    MoveWindow(gRouteStatusButton, actionX, networkOffset + 218, fiveButtonWidth, 30, TRUE);
    actionX += fiveButtonWidth + actionGap;
    MoveWindow(gNodeListButton, actionX, networkOffset + 218, fiveButtonWidth, 30, TRUE);
    actionX += fiveButtonWidth + actionGap;
    MoveWindow(gDaemonStatusButton, actionX, networkOffset + 218,
               std::max(76, pageWidth - padding - actionX), 30, TRUE);
    MoveWindow(gPages[2].log, padding, networkOffset + 258, pageWidth - padding * 2,
               std::max(40, pageHeight - (networkOffset + 272)), TRUE);

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

    // Registration requests are grouped by canonical app id in the daemon;
    // only the newest request for each app is ever actionable here.
    const int appActionGap = 8;
    const int appActionWidth = std::max(108, (pageWidth - padding * 2 - appActionGap * 3) / 4);
    int appActionX = padding;
    MoveWindow(gAppPendingButton, appActionX, 12, appActionWidth, 30, TRUE); appActionX += appActionWidth + appActionGap;
    MoveWindow(gAppApproveButton, appActionX, 12, appActionWidth, 30, TRUE); appActionX += appActionWidth + appActionGap;
    MoveWindow(gAppRejectButton, appActionX, 12, appActionWidth, 30, TRUE); appActionX += appActionWidth + appActionGap;
    MoveWindow(gAppAdvancedButton, appActionX, 12, std::max(108, pageWidth - padding - appActionX), 30, TRUE);
    MoveWindow(gAppRequestList, padding, 50, pageWidth - padding * 2, 104, TRUE);
    MoveWindow(gAppFoundLabel, padding, 160, pageWidth - padding * 2, 22, TRUE);
    MoveWindow(gAppFoundList, padding, 184, pageWidth - padding * 2, 94, TRUE);

    const int advancedY = 286;
    MoveWindow(gAppIdLabel, padding, advancedY + 6, 86, 22, TRUE);
    MoveWindow(gAppIdEdit, 104, advancedY + 1, 146, rowHeight, TRUE);
    MoveWindow(gAppNameLabel, 260, advancedY + 6, 92, 22, TRUE);
    MoveWindow(gAppNameEdit, 356, advancedY + 1, std::max(90, pageWidth - 480), rowHeight, TRUE);
    MoveWindow(gAppAddButton, pageWidth - 112, advancedY, 98, 30, TRUE);
    MoveWindow(gAppListButton, padding, advancedY + 35, 98, 30, TRUE);
    MoveWindow(gAppRotateButton, 120, advancedY + 35, 100, 30, TRUE);
    MoveWindow(gAppReasonLabel, 228, advancedY + 41, 92, 22, TRUE);
    MoveWindow(gAppReasonEdit, 324, advancedY + 36, std::max(80, pageWidth - 438), rowHeight, TRUE);
    MoveWindow(gAppRejectButton, pageWidth - 110, advancedY + 35, 96, 30, TRUE);

    const int aliasWidth = std::max(92, (pageWidth - padding * 2 - 24) / 4);
    int aliasX = padding;
    MoveWindow(gAppVisibleDefaultButton, aliasX, advancedY + 72, aliasWidth, 30, TRUE); aliasX += aliasWidth + 8;
    MoveWindow(gAppVisibleAliasButton, aliasX, advancedY + 72, aliasWidth, 30, TRUE); aliasX += aliasWidth + 8;
    MoveWindow(gAppVisibleClearButton, aliasX, advancedY + 72, aliasWidth, 30, TRUE); aliasX += aliasWidth + 8;
    MoveWindow(gAppVisibleListButton, aliasX, advancedY + 72, std::max(92, pageWidth - padding - aliasX), 30, TRUE);

    MoveWindow(gProfileNameLabel, padding, advancedY + 112, 112, 22, TRUE);
    MoveWindow(gProfileNameEdit, 130, advancedY + 107, std::max(100, pageWidth / 2 - 144), rowHeight, TRUE);
    MoveWindow(gProfileIdLabel, pageWidth / 2, advancedY + 112, 70, 22, TRUE);
    MoveWindow(gProfileIdEdit, pageWidth / 2 + 74, advancedY + 107,
               std::max(100, pageWidth / 2 - 88), rowHeight, TRUE);

    const int profileWidth = std::max(92, (pageWidth - padding * 2 - 24) / 4);
    int profileX = padding;
    MoveWindow(gProfileCreateButton, profileX, advancedY + 142, profileWidth, 30, TRUE); profileX += profileWidth + 8;
    MoveWindow(gProfileListButton, profileX, advancedY + 142, profileWidth, 30, TRUE); profileX += profileWidth + 8;
    MoveWindow(gProfileUseButton, profileX, advancedY + 142, profileWidth, 30, TRUE); profileX += profileWidth + 8;
    MoveWindow(gProfileRetireButton, profileX, advancedY + 142, std::max(92, pageWidth - padding - profileX), 30, TRUE);

    HWND advancedControls[] = {
        gAppIdLabel, gAppIdEdit, gAppNameLabel, gAppNameEdit, gAppAddButton,
        gAppListButton, gAppRotateButton, gAppReasonLabel, gAppReasonEdit,
        gAppVisibleDefaultButton, gAppVisibleAliasButton,
        gAppVisibleClearButton, gAppVisibleListButton, gProfileNameLabel,
        gProfileNameEdit, gProfileIdLabel, gProfileIdEdit, gProfileCreateButton,
        gProfileListButton, gProfileUseButton, gProfileRetireButton
    };
    for (HWND control : advancedControls) {
        ShowWindow(control, gAppAdvancedExpanded ? SW_SHOW : SW_HIDE);
    }

    ShowWindow(gAppRequestLabel, SW_HIDE);
    ShowWindow(gAppRequestEdit, SW_HIDE);
    const int appLogTop = gAppAdvancedExpanded ? advancedY + 180 : 286;
    MoveWindow(gPages[6].log, padding, appLogTop, pageWidth - padding * 2,
               std::max(40, pageHeight - appLogTop - 14), TRUE);

    // Dedicated encrypted-backup and network-recovery page.
    MoveWindow(gBackupPassphraseLabel, padding, 18, 104, 22, TRUE);
    MoveWindow(gBackupPassphraseEdit, 122, 13, 220, rowHeight, TRUE);
    MoveWindow(gBackupLocalButton, 350, 12, 128, 30, TRUE);
    MoveWindow(gBackupUploadButton, 486, 12, std::max(150, pageWidth - 500), 30, TRUE);

    MoveWindow(gBackupPathLabel, padding, 58, 82, 22, TRUE);
    MoveWindow(gBackupPathEdit, 100, 53, std::max(150, pageWidth - 210), rowHeight, TRUE);
    MoveWindow(gBackupCopyPathButton, pageWidth - 96, 52, 82, 30, TRUE);

    MoveWindow(gBackupRecoveryCodeLabel, padding, 94, 104, 22, TRUE);
    MoveWindow(gBackupRecoveryCodeEdit, 122, 89, std::max(150, pageWidth - 218), rowHeight, TRUE);
    MoveWindow(gBackupCopyCodeButton, pageWidth - 96, 88, 82, 30, TRUE);

    const int backupActionGap = 8;
    const int backupActionWidth = std::max(110, (pageWidth - padding * 2 - backupActionGap * 2) / 3);
    int backupX = padding;
    MoveWindow(gBackupDownloadButton, backupX, 126, backupActionWidth, 30, TRUE); backupX += backupActionWidth + backupActionGap;
    MoveWindow(gBackupStatusButton, backupX, 126, backupActionWidth, 30, TRUE); backupX += backupActionWidth + backupActionGap;
    MoveWindow(gBackupWipeButton, backupX, 126, std::max(110, pageWidth - padding - backupX), 30, TRUE);

    MoveWindow(gPages[8].log, padding, 170, pageWidth - padding * 2,
               std::max(40, pageHeight - 184), TRUE);
    MoveWindow(gPages[7].log, padding, padding, pageWidth - padding * 2,
               pageHeight - padding * 2, TRUE);
}

void ShowSelectedPage() {
    static constexpr int kTabPageMap[] = {6, 8, 0, 1, 2, 3, 4, 5, 7};
    const int selected = TabCtrl_GetCurSel(gTab);
    const int selectedPage = (selected >= 0 && selected < 9) ? kTabPageMap[selected] : 0;
    for (size_t index = 0; index < gPages.size(); ++index) {
        ShowWindow(gPages[index].container, static_cast<int>(index) == selectedPage ? SW_SHOW : SW_HIDE);
    }
}

void DrawOwnerControl(const DRAWITEMSTRUCT* draw) {
    if (draw->CtlID == ID_LANGUAGE) {
        const bool selected = (draw->itemState & ODS_SELECTED) != 0;
        const bool focused = (draw->itemState & ODS_FOCUS) != 0;
        HBRUSH background = CreateSolidBrush(selected ? kColorRedDark : kColorEdit);
        FillRect(draw->hDC, &draw->rcItem, background);
        DeleteObject(background);
        UINT item = draw->itemID;
        if (item == static_cast<UINT>(-1)) {
            const LRESULT current = SendMessageW(draw->hwndItem, CB_GETCURSEL, 0, 0);
            if (current >= 0) item = static_cast<UINT>(current);
        }
        if (item != static_cast<UINT>(-1)) {
            wchar_t text[128]{};
            SendMessageW(draw->hwndItem, CB_GETLBTEXT, item,
                         reinterpret_cast<LPARAM>(text));
            SetBkMode(draw->hDC, TRANSPARENT);
            SetTextColor(draw->hDC, kColorText);
            SelectObject(draw->hDC, gUiFont);
            RECT textRect = draw->rcItem;
            textRect.left += 8;
            DrawTextW(draw->hDC, text, -1, &textRect,
                      DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        }
        if (focused) DrawFocusRect(draw->hDC, &draw->rcItem);
        return;
    }
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
        L"Applications", L"Backup", L"Overview", L"Handshake", L"Network",
        L"Headers", L"DHT", L"Mailbox", L"All Logs"
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
    gRestoreBackupButton = CreateButton(overview, L"Restore backup", ID_RESTORE_BACKUP);
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
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS | WS_VSCROLL,
        0, 0, 10, 10, overview,
        reinterpret_cast<HMENU>(static_cast<INT_PTR>(ID_LANGUAGE)), gInstance, nullptr);
    ApplyFont(gLanguageCombo);
    SetWindowTheme(gLanguageCombo, L"DarkMode_Explorer", nullptr);
    for (int index = 0; index < 5; ++index) {
        SendMessageW(gLanguageCombo, CB_ADDSTRING, 0,
                     reinterpret_cast<LPARAM>(UiLanguageNativeName(static_cast<UiLanguage>(index))));
    }
    SendMessageW(gLanguageCombo, CB_SETCURSEL, static_cast<WPARAM>(gLanguage), 0);

    HWND backup = gPages[8].container;
    gBackupPassphraseLabel = CreateLabel(backup, L"Backup passphrase");
    gBackupPassphraseEdit = CreateEdit(backup, ID_BACKUP_PASSPHRASE, ES_PASSWORD);
    gBackupLocalButton = CreateButton(backup, L"Create local backup", ID_BACKUP_LOCAL);
    gBackupUploadButton = CreateButton(backup, L"Create backup and upload recovery copy", ID_BACKUP_UPLOAD);
    gBackupPathLabel = CreateLabel(backup, L"Backup file");
    gBackupPathEdit = CreateEdit(backup, ID_BACKUP_PATH, ES_READONLY);
    gBackupCopyPathButton = CreateButton(backup, L"Copy backup path", ID_BACKUP_COPY_PATH);
    gBackupRecoveryCodeLabel = CreateLabel(backup, L"Recovery code");
    gBackupRecoveryCodeEdit = CreateEdit(backup, ID_BACKUP_RECOVERY_CODE);
    gBackupCopyCodeButton = CreateButton(backup, L"Copy recovery code", ID_BACKUP_COPY_CODE);
    gBackupDownloadButton = CreateButton(backup, L"Download recovery backup", ID_BACKUP_DOWNLOAD);
    gBackupStatusButton = CreateButton(backup, L"Recovery status", ID_BACKUP_STATUS);
    gBackupWipeButton = CreateButton(backup, L"Wipe network recovery", ID_BACKUP_WIPE);
    EnableBackupControls(false);

    gOldUsernameProc = reinterpret_cast<WNDPROC>(SetWindowLongPtrW(gUsernameEdit, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(CredentialEditProc)));
    gOldPasswordProc = reinterpret_cast<WNDPROC>(SetWindowLongPtrW(gPasswordEdit, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(CredentialEditProc)));

    HWND handshake = gPages[1].container;
    gHandshakeLabel = CreateLabel(handshake, L"Peer VLD0 key");
    gHandshakeEdit = CreateEdit(handshake, ID_HANDSHAKE_KEY);
    gHandshakeStartButton = CreateButton(handshake, L"Establish handshake", ID_HANDSHAKE_START);
    gHandshakeStatusButton = CreateButton(handshake, L"Check status", ID_HANDSHAKE_STATUS);
    gHandshakeResultLabel = CreateLabel(handshake, L"No handshake action requested yet.");

    HWND network = gPages[2].container;
    gSummaryTopologyGroup = CreateGroup(network, L"Topology");
    gSummaryTopologyText = CreateLabel(network, L"Verified: 0\r\nCandidates: 0\r\nAuthenticated: 0");
    gSummaryPresenceGroup = CreateGroup(network, L"Presence");
    gSummaryPresenceText = CreateLabel(network, L"Online: 0\r\nOffline: 0\r\nStale claim: 0\r\nNeeds refresh: 0\r\nUnknown: 0");
    gSummaryHeadersGroup = CreateGroup(network, L"Header cache");
    gSummaryHeadersText = CreateLabel(network, L"Presence OK: 0\r\nRead failed: 0\r\nNot checked: 0\r\nActive app info: 0\r\nMailbox capable: 0");
    gSummaryActivityGroup = CreateGroup(network, L"Activity");
    gSummaryActivityText = CreateLabel(network, L"Walk: idle\r\nNew / updated: 0 / 0\r\nReach / fail: 0 / 0\r\nApp searches: 0\r\nRoot lookups: 0");

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
    gAppPendingButton = CreateButton(applications, L"Refresh requests", ID_APP_PENDING);
    gAppRequestList = CreateWindowExW(WS_EX_CLIENTEDGE, WC_LISTVIEWW, L"",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | LVS_REPORT | LVS_SHOWSELALWAYS,
        0, 0, 10, 10, applications,
        reinterpret_cast<HMENU>(static_cast<INT_PTR>(ID_APP_REQUEST_LIST)), gInstance, nullptr);
    ApplyFont(gAppRequestList);
    ListView_SetExtendedListViewStyle(gAppRequestList,
        LVS_EX_CHECKBOXES | LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER);
    ListView_SetBkColor(gAppRequestList, kColorEdit);
    ListView_SetTextBkColor(gAppRequestList, kColorEdit);
    ListView_SetTextColor(gAppRequestList, kColorText);
    SetWindowTheme(gAppRequestList, L"DarkMode_Explorer", nullptr);
    for (const auto& column : std::vector<std::pair<const wchar_t*, int>>{
        {L"Application", 260}, {L"Display name", 210}, {L"Request", 90}}) {
        LVCOLUMNW item{}; item.mask = LVCF_TEXT | LVCF_WIDTH;
        item.pszText = const_cast<wchar_t*>(column.first); item.cx = column.second;
        ListView_InsertColumn(gAppRequestList, Header_GetItemCount(ListView_GetHeader(gAppRequestList)), &item);
    }
    gAppApproveButton = CreateButton(applications, L"Allow checked", ID_APP_APPROVE);
    gAppRejectButton = CreateButton(applications, L"Refuse checked", ID_APP_REJECT);
    gAppFoundLabel = CreateLabel(applications, L"Observed applications");
    gAppFoundList = CreateWindowExW(WS_EX_CLIENTEDGE, WC_LISTVIEWW, L"",
        WS_CHILD | WS_VISIBLE | LVS_REPORT | LVS_SHOWSELALWAYS,
        0, 0, 10, 10, applications,
        reinterpret_cast<HMENU>(static_cast<INT_PTR>(ID_APP_FOUND_LIST)), gInstance, nullptr);
    ApplyFont(gAppFoundList);
    ListView_SetExtendedListViewStyle(gAppFoundList, LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER);
    ListView_SetBkColor(gAppFoundList, kColorEdit);
    ListView_SetTextBkColor(gAppFoundList, kColorEdit);
    ListView_SetTextColor(gAppFoundList, kColorText);
    SetWindowTheme(gAppFoundList, L"DarkMode_Explorer", nullptr);
    for (const auto& column : std::vector<std::pair<const wchar_t*, int>>{
        {L"Application", 260}, {L"Verified headers", 105}, {L"Discovery cache", 110},
        {L"Recent", 70}, {L"Archive", 70}}) {
        LVCOLUMNW item{}; item.mask = LVCF_TEXT | LVCF_WIDTH;
        item.pszText = const_cast<wchar_t*>(column.first); item.cx = column.second;
        ListView_InsertColumn(gAppFoundList, Header_GetItemCount(ListView_GetHeader(gAppFoundList)), &item);
    }
    // Legacy request-id controls remain allocated only for compatibility with
    // older layout code; the visible workflow is the checked request list.
    gAppRequestLabel = CreateLabel(applications, L"Request id");
    gAppRequestEdit = CreateEdit(applications, ID_APP_REQUEST);
    ShowWindow(gAppRequestLabel, SW_HIDE);
    ShowWindow(gAppRequestEdit, SW_HIDE);
    gAppReasonLabel = CreateLabel(applications, L"Reject reason");
    gAppReasonEdit = CreateEdit(applications, ID_APP_REASON);
    gAppVisibleDefaultButton = CreateButton(applications, L"Set default visible name", ID_APP_NAME_DEFAULT);
    gAppVisibleAliasButton = CreateButton(applications, L"Set app alias", ID_APP_NAME_ALIAS);
    gAppVisibleClearButton = CreateButton(applications, L"Clear alias", ID_APP_NAME_CLEAR);
    gAppVisibleListButton = CreateButton(applications, L"List aliases", ID_APP_NAME_LIST);
    gAppAdvancedButton = CreateButton(applications, L"Advanced application management ▸", ID_APP_ADVANCED);
    gProfileNameLabel = CreateLabel(applications, L"New profile name");
    gProfileNameEdit = CreateEdit(applications, ID_PROFILE_NAME);
    gProfileIdLabel = CreateLabel(applications, L"Profile id");
    gProfileIdEdit = CreateEdit(applications, ID_PROFILE_ID);
    gProfileCreateButton = CreateButton(applications, L"Create profile", ID_PROFILE_CREATE);
    gProfileListButton = CreateButton(applications, L"List profiles", ID_PROFILE_LIST);
    gProfileUseButton = CreateButton(applications, L"Use after restart", ID_PROFILE_USE);
    gProfileRetireButton = CreateButton(applications, L"Retire profile", ID_PROFILE_RETIRE);

    // Keep the login form visible until authentication succeeds. Once the
    // account is authenticated, ProcessLogLine switches to Applications (tab 0).
    TabCtrl_SetCurSel(gTab, 2);
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
    case WM_MEASUREITEM: {
        auto* measure = reinterpret_cast<MEASUREITEMSTRUCT*>(lParam);
        if (measure && measure->CtlID == ID_LANGUAGE) {
            measure->itemHeight = 24;
            return TRUE;
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
        case ID_RESTORE_BACKUP: RestoreLocalBackup(); return 0;
        case ID_COPY_KEY: CopyMainKey(); return 0;
        case ID_SAVE_LOG: SaveSessionLog(); return 0;
        case ID_SHUTDOWN: BeginProperShutdown(); return 0;
        case ID_HELP: ShowHelp(); return 0;
        case ID_DISCORD:
            ShellExecuteW(window, L"open", L"https://discord.gg/yy5SMTuZY", nullptr, nullptr, SW_SHOWNORMAL);
            return 0;
        case ID_LANGUAGE:
            if (HIWORD(wParam) == CBN_SELCHANGE) {
                const LRESULT selected = SendMessageW(gLanguageCombo, CB_GETCURSEL, 0, 0);
                if (selected >= 0 && selected <= 4) {
                    gLanguage = static_cast<UiLanguage>(selected);
                    SaveUiLanguage();
                    ApplyLanguage();
                    RefreshWindowPaint();
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
        case ID_BACKUP_LOCAL:
        case ID_BACKUP_UPLOAD:
        case ID_BACKUP_COPY_PATH:
        case ID_BACKUP_COPY_CODE:
        case ID_BACKUP_DOWNLOAD:
        case ID_BACKUP_STATUS:
        case ID_BACKUP_WIPE:
            HandleBackupAction(id); return 0;
        case ID_APP_ADVANCED:
            gAppAdvancedExpanded = !gAppAdvancedExpanded;
            ApplyLanguage();
            {
                RECT client{};
                GetClientRect(window, &client);
                LayoutPages(client.right - client.left, client.bottom - client.top);
            }
            return 0;
        case ID_APP_ADD:
        case ID_APP_LIST:
        case ID_APP_ROTATE:
        case ID_APP_PENDING:
        case ID_APP_APPROVE:
        case ID_APP_REJECT:
        case ID_APP_NAME_DEFAULT:
        case ID_APP_NAME_ALIAS:
        case ID_APP_NAME_CLEAR:
        case ID_APP_NAME_LIST:
        case ID_PROFILE_CREATE:
        case ID_PROFILE_LIST:
        case ID_PROFILE_USE:
        case ID_PROFILE_RETIRE:
            HandleApplicationAction(id); return 0;
        case ID_TRAY_OPEN: RestoreFromTray(); return 0;
        case ID_TRAY_PROPER: BeginProperShutdown(); return 0;
        case ID_TRAY_CRAZY: ForceClose(); return 0;
        default: break;
        }
        break;
    }
    case WM_TIMER:
        if (wParam == ID_SUMMARY_TIMER && gReady) {
            SendBackendLine(L"summary");
            SendBackendLine(L"app-pending");
            return 0;
        }
        break;
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
            EnableBackupControls(false);
            gAuthenticated = false;
            gReady = false;
            TabCtrl_SetCurSel(gTab, 2);
            ShowSelectedPage();
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
        KillTimer(window, ID_SUMMARY_TIMER);
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
    RefreshWindowPaint();
    SetTimer(gMainWindow, ID_SUMMARY_TIMER, 15000, nullptr);

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

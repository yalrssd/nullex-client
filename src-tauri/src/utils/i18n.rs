use std::sync::RwLock;

static CURRENT_LANG: RwLock<&'static str> = RwLock::new("ru");

pub fn set_language(lang: &str) {
    let normalized = if lang.to_lowercase().starts_with("ru") {
        "ru"
    } else {
        "en"
    };
    if let Ok(mut g) = CURRENT_LANG.write() {
        *g = normalized;
    }
}

pub fn current_lang() -> &'static str {
    CURRENT_LANG.read().map(|g| *g).unwrap_or("ru")
}

pub fn tr(key: &str) -> &'static str {
    let ru = current_lang() == "ru";
    match key {
        "error.subscriptionFormatError" => {
            if ru {
                "Неверный формат подписки: не удалось разобрать конфигурационный файл"
            } else {
                "Invalid subscription format: failed to parse configuration file"
            }
        }
        "error.duplicateProfile" => {
            if ru {
                "Профиль с таким URL уже существует"
            } else {
                "Profile with this URL already exists"
            }
        }
        "error.coreUnreachable" => {
            if ru {
                "Ядро не отвечает"
            } else {
                "Core is unreachable"
            }
        }
        "error.serviceMayNotInstalled" => {
            if ru {
                "Служба не установлена или не запущена"
            } else {
                "Service may not be installed or running"
            }
        }
        "error.corePathError" => {
            if ru {
                "Неподдерживаемый тип ядра"
            } else {
                "Unsupported core path"
            }
        }
        "error.autoUpdateNotSupported" => {
            if ru {
                "Автообновление не поддерживается на данной платформе"
            } else {
                "Auto update not supported on this platform"
            }
        }
        "tray.showWindow" => {
            if ru {
                "Показать окно"
            } else {
                "Show Window"
            }
        }
        "tray.quit" => {
            if ru {
                "Выход"
            } else {
                "Quit"
            }
        }
        "tray.sysProxy" => {
            if ru {
                "Системный прокси"
            } else {
                "System Proxy"
            }
        }
        "tray.tun" => {
            if ru {
                "TUN режим"
            } else {
                "TUN Mode"
            }
        }
        "tray.ruleMode" => {
            if ru {
                "Правила"
            } else {
                "Rule"
            }
        }
        "tray.globalMode" => {
            if ru {
                "Глобальный"
            } else {
                "Global"
            }
        }
        "tray.directMode" => {
            if ru {
                "Прямой"
            } else {
                "Direct"
            }
        }
        "tray.copyEnv" => {
            if ru {
                "Скопировать прокси-переменные"
            } else {
                "Copy Env"
            }
        }
        "tray.coreStartError" => {
            if ru {
                "Ошибка запуска ядра"
            } else {
                "Core start failed"
            }
        }
        "ui.blankSubscription" => {
            if ru {
                "Пустой профиль"
            } else {
                "Blank Subscription"
            }
        }
        "ui.defaultTheme" => {
            if ru {
                "По умолчанию"
            } else {
                "Default"
            }
        }
        _ => "",
    }
}

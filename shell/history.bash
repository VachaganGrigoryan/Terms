# Source explicitly. Automatic capture requires Bash 4.4+.
_th_hex() {
    local text=$1 i LC_ALL=C
    for ((i=0; i<${#text}; i++)); do printf '%02x' "'${text:i:1}"; done
    return 0
}
_th_notice() {
    if [ "${_TH_STATE-}" != "$1" ]; then printf 'terms history: %s\n' "$1" >&2; _TH_STATE=$1; fi
}
_th_debug() {
    local rc=$? command cwdhex commandhex result
    [ "${_TH_ENABLED:-0}" = 1 ] && [ "${_TH_READY:-0}" = 1 ] || return "$rc"
    case $BASH_COMMAND in _th_*|terms_history_*) return "$rc";; esac
    _TH_READY=0; _TH_PENDING=
    # fc emits one formatting tab and a final newline; sentinel preserves trailing bytes.
    command=$(builtin fc -ln -1; printf '.')
    command=${command%.}; command=${command%$'\n'}; command=${command#$'\t'}
    case $command in ''|' '*|terms_history_*|_th_*) return "$rc";; esac
    local LC_ALL=C
    if [ "${#command}" -gt 65536 ] || [ "${#PWD}" -gt 16384 ]; then _th_notice 'record exceeds capture limit'; return "$rc"; fi
    cwdhex=$(_th_hex "$PWD"); commandhex=$(_th_hex "$command")
    result=$(printf '%s\n%s\n%s\n' "$_TH_SESSION" "$cwdhex" "$commandhex" | "$_TH_BIN" history begin 2>/dev/null)
    local result_status=$?
    if [ "$result_status" -eq 0 ] && [ "$result" = excluded ]; then _th_notice excluded; return "$rc"; fi
    if [ "$result_status" -eq 0 ] && [[ $result =~ ^[0-9a-f]+$ ]]; then _TH_PENDING=$result; _th_notice recording
    else _th_notice 'locked or recording failed'; fi
    return "$rc"
}
_th_prompt() {
    local rc=$?
    _TH_READY=0
    if [ -n "${_TH_PENDING-}" ]; then
        if printf '%s\n%s\n%s\n' "$_TH_SESSION" "$_TH_PENDING" "$rc" | "$_TH_BIN" history finish >/dev/null 2>&1; then _th_notice recording
        else _th_notice 'locked or recording failed'; fi
        _TH_PENDING=
    fi
    return "$rc"
}
_th_ready() { local rc=$?; _TH_READY=1; return "$rc"; }
_th_exit() { local rc=$?; printf '%s\n' "${_TH_SESSION-}" | "$_TH_BIN" history session-close >/dev/null 2>&1; return "$rc"; }
terms_history_enable() {
    [ "${_TH_ENABLED:-0}" != 1 ] || return 0
    if [ "${BASH_VERSINFO[0]}" -lt 4 ] || { [ "${BASH_VERSINFO[0]}" -eq 4 ] && [ "${BASH_VERSINFO[1]}" -lt 4 ]; }; then
        printf '%s\n' 'terms history: automatic capture requires Bash 4.4+; history CLI remains available' >&2; return 2
    fi
    case $- in *i*) ;; *) printf '%s\n' 'terms history: capture requires interactive shell' >&2; return 2;; esac
    case :$SHELLOPTS: in *:functrace:*) printf '%s\n' 'terms history: functrace is incompatible with capture' >&2; return 2;; esac
    shopt -q extdebug && { printf '%s\n' 'terms history: extdebug is incompatible with capture' >&2; return 2; }
    [ -z "$(trap -p DEBUG)" ] && [ -z "$(trap -p EXIT)" ] || { printf '%s\n' 'terms history: existing DEBUG/EXIT trap unsupported' >&2; return 2; }
    case $(declare -p PROMPT_COMMAND 2>/dev/null) in 'declare -a'*) return 2;; esac
    case $(declare -p HISTFILE 2>/dev/null) in 'declare -r'*|'declare -'*r*' HISTFILE='*) return 2;; esac
    _TH_BIN=${TERMS_BIN:-$(type -P terms)}
    if [ "${_TH_BIN#/}" = "$_TH_BIN" ] || [ ! -x "$_TH_BIN" ]; then
        if [ -n "${TERMS_BIN-}" ]; then
            printf 'terms history: TERMS_BIN=%q is not an absolute path to an executable; fix it, or run: unset TERMS_BIN\n' "$TERMS_BIN" >&2
        else
            printf '%s\n' 'terms history: cannot find terms on PATH; set TERMS_BIN to its absolute path' >&2
        fi
        return 2
    fi
    local session
    session=$(printf '%s\n' "$$" | "$_TH_BIN" history session-open) || return
    [[ $session =~ ^[0-9a-f]+$ ]] || return 2
    unset HISTFILE
    # Ensure fc refers to the actual submitted line, including excluded/duplicate lines.
    _TH_OLD_HISTCONTROL=${HISTCONTROL-}; _TH_OLD_HISTIGNORE=${HISTIGNORE-}
    HISTCONTROL=; HISTIGNORE=
    set -o history
    shopt -s cmdhist lithist
    _TH_SESSION=$session; _TH_PENDING=; _TH_ENABLED=1; _TH_READY=0
    _TH_OLD_PROMPT=${PROMPT_COMMAND-}
    if [ -n "$_TH_OLD_PROMPT" ]; then _TH_OWN_PROMPT='_th_prompt; '$_TH_OLD_PROMPT'; _th_ready'; else _TH_OWN_PROMPT='_th_prompt; _th_ready'; fi
    PROMPT_COMMAND=$_TH_OWN_PROMPT
    trap '_th_debug' DEBUG
    trap '_th_exit' EXIT
    printf '%s\n' 'terms history: capture enabled; native file saving stays disabled until shell exit' >&2
}
terms_history_disable() {
    _TH_ENABLED=0; _TH_READY=0
    [ "${PROMPT_COMMAND-}" != "${_TH_OWN_PROMPT-unset}" ] || PROMPT_COMMAND=$_TH_OLD_PROMPT
    case $(trap -p DEBUG) in *"'_th_debug'"*) trap - DEBUG;; esac
    case $(trap -p EXIT) in *"'_th_exit'"*) trap - EXIT;; esac
    _th_exit
    terms_history_bindings_disable
    HISTCONTROL=${_TH_OLD_HISTCONTROL-}; HISTIGNORE=${_TH_OLD_HISTIGNORE-}
    unset _TH_SESSION _TH_PENDING _TH_STATE
    # Never restore HISTFILE while managed commands remain in memory.
}
_th_recall() {
    local encoded pair escapes= command
    encoded=$("$_TH_BIN" history pick --hex) || return
    [[ $encoded =~ ^([0-9a-f][0-9a-f])+$ ]] || return 2
    while [ -n "$encoded" ]; do
        pair=${encoded:0:2}; encoded=${encoded:2}
        [ "$pair" != 00 ] || return 2
        escapes=$escapes\\x$pair
    done
    printf -v command '%b' "$escapes"
    READLINE_LINE=$command; READLINE_POINT=${#command}
}
terms_history_bindings_enable() {
    [ "${_TH_ENABLED:-0}" = 1 ] || return 2
    local bindings
    bindings=$(bind -p; bind -s; bind -X)
    case $bindings in *'"\C-r":'*) printf '%s\n' 'terms history: Ctrl-R occupied; unbind explicitly first' >&2; return 2;; esac
    bind -x '"\C-r":_th_recall'
}
terms_history_bindings_disable() {
    case $(bind -X 2>/dev/null) in *'"\C-r": "_th_recall"'*) bind -r '\C-r';; esac
}

# Source explicitly. Capture starts only after terms_history_enable.
_th_hex() {
    local text=$1 i LC_ALL=C
    for ((i=1; i<=${#text}; i++)); do printf '%02x' "'${text[i]}"; done
    return 0
}
_th_notice() {
    local state=$1
    if [[ ${_TH_STATE-} != $state ]]; then print -u2 -- "terms history: $state"; _TH_STATE=$state; fi
}
_th_preexec() {
    [[ ${_TH_ENABLED:-0} = 1 ]] || return 0
    local command=$1 cwdhex commandhex result rc
    _TH_PENDING=''
    case $command in ' '*|terms_history_*|_th_*) return 0;; esac
    local LC_ALL=C
    if [ "${#command}" -gt 65536 ] || [ "${#PWD}" -gt 16384 ]; then _th_notice 'record exceeds capture limit'; return 0; fi
    cwdhex=$(_th_hex "$PWD"); commandhex=$(_th_hex "$command")
    result=$(printf '%s\n%s\n%s\n' "$_TH_SESSION" "$cwdhex" "$commandhex" | "$_TH_BIN" history begin 2>/dev/null)
    rc=$?
    if (( rc == 0 )) && [[ $result = excluded ]]; then _th_notice excluded; return 0; fi
    if (( rc == 0 )) && [[ -n $result && $result != *[^0123456789abcdef]* ]]; then
        _TH_PENDING=$result; _th_notice recording
    else _th_notice 'locked or recording failed'; fi
    return 0
}
_th_precmd() {
    local rc=$?
    if [[ -n ${_TH_PENDING-} ]]; then
        if printf '%s\n%s\n%s\n' "$_TH_SESSION" "$_TH_PENDING" "$rc" | "$_TH_BIN" history finish >/dev/null 2>&1; then
            _th_notice recording
        else _th_notice 'locked or recording failed'; fi
        _TH_PENDING=''
    fi
    return $rc
}
_th_exit() {
    local rc=$?
    [[ -z ${_TH_SESSION-} ]] || printf '%s\n' "$_TH_SESSION" | "$_TH_BIN" history session-close >/dev/null 2>&1
    return $rc
}
terms_history_enable() {
    [[ ${_TH_ENABLED:-0} != 1 ]] || return 0
    [[ -o interactive ]] || { print -u2 'terms history: capture requires an interactive shell'; return 2; }
    _TH_BIN=${TERMS_BIN:-${commands[terms]-}}
    [[ -n $_TH_BIN ]] || _TH_BIN=$(whence -p terms 2>/dev/null)
    if [[ $_TH_BIN != /* || ! -x $_TH_BIN ]]; then
        if [[ -n ${TERMS_BIN-} ]]; then
            print -u2 -r -- "terms history: TERMS_BIN=${(q)TERMS_BIN} is not an absolute path to an executable; fix it, or run: unset TERMS_BIN"
        else
            print -u2 'terms history: cannot find terms on PATH; set TERMS_BIN to its absolute path'
        fi
        return 2
    fi
    # A readonly history target cannot be disabled safely.
    [[ ${parameters[HISTFILE]-} != *readonly* ]] || { print -u2 'terms history: readonly HISTFILE'; return 2; }
    local session
    session=$(printf '%s\n' "$$" | "$_TH_BIN" history session-open) || return
    [[ -n $session && $session != *[^0123456789abcdef]* ]] || return 2
    unset HISTFILE
    _TH_SESSION=$session; _TH_ENABLED=1; _TH_PENDING=''
    typeset -ga preexec_functions precmd_functions zshexit_functions
    preexec_functions=(_th_preexec ${preexec_functions:#_th_preexec})
    precmd_functions=(_th_precmd ${precmd_functions:#_th_precmd})
    zshexit_functions=(${zshexit_functions:#_th_exit} _th_exit)
    print -u2 'terms history: capture enabled; native file saving stays disabled until this shell exits'
}
terms_history_disable() {
    _TH_ENABLED=0
    preexec_functions=(${preexec_functions:#_th_preexec})
    precmd_functions=(${precmd_functions:#_th_precmd})
    zshexit_functions=(${zshexit_functions:#_th_exit})
    _th_exit
    terms_history_bindings_disable
    unset _TH_SESSION _TH_PENDING _TH_STATE
    # Deliberately never restore HISTFILE: the buffer may contain private commands.
}
_th_recall() {
    local encoded pair escapes='' command
    encoded=$("$_TH_BIN" history pick --hex) || return
    [[ -n $encoded && $encoded != *[^0123456789abcdef]* && $((${#encoded} % 2)) = 0 ]] || return 2
    while [[ -n $encoded ]]; do
        pair=${encoded[1,2]}; encoded=${encoded[3,-1]}
        [[ $pair != 00 ]] || return 2
        escapes+="\\x$pair"
    done
    printf -v command '%b' "$escapes"
    BUFFER=$command; CURSOR=${#BUFFER}
}
terms_history_bindings_enable() {
    [[ ${_TH_ENABLED:-0} = 1 ]] || return 2
    [[ $(bindkey '^R') = *undefined-key* ]] || { print -u2 'terms history: Ctrl-R occupied; unbind explicitly first'; return 2; }
    zle -N _th_recall; bindkey '^R' _th_recall
}
terms_history_bindings_disable() {
    [[ $(bindkey '^R') != *'_th_recall' ]] || bindkey -r '^R'
}

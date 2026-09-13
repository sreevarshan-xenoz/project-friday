# FRIDAY OS — Cognitive Operating System Environment
# Substrate: Omarchy / Arch Linux

# History setup
HISTFILE=~/.zsh_history
HISTSIZE=10000
SAVEHIST=10000
setopt APPEND_HISTORY
setopt SHARE_HISTORY
setopt HIST_IGNORE_DUPS
setopt HIST_IGNORE_ALL_DUPS
setopt AUTO_CD

# Environment
export EDITOR=nano
export VISUAL=nano
export PAGER=less
export PATH="$HOME/.local/bin:$HOME/bin:/usr/local/bin:$PATH"

# Aliases
alias ll='ls -lah --color=auto'
alias la='ls -A --color=auto'
alias l='ls -CF --color=auto'
alias ls='ls --color=auto'
alias grep='grep --color=auto'
alias diff='diff --color=auto'
alias update='sudo pacman -Syu'
alias friday='friday-ctl'

# Prompt initialization (Starship)
if command -v starship >/dev/null 2>&1; then
    eval "$(starship init zsh)"
else
    PROMPT='%F{blue}%n%f@%F{magenta}%m%f:%F{cyan}%~%f$ '
fi

# Fastfetch greeting on interactive shell
if [[ -o interactive ]] && command -v fastfetch >/dev/null 2>&1 && [[ -z "$FASTFETCH_SHOWN" ]]; then
    export FASTFETCH_SHOWN=1
    fastfetch --logo arch_small 2>/dev/null || true
fi

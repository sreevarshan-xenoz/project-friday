hl.config({
    general = {
        col = {
            active_border   = "rgba({{colors.primary.default.hex_stripped}}ee) rgba({{colors.secondary.default.hex_stripped}}ee) 45deg",
            inactive_border = "rgba({{colors.outline.default.hex_stripped}}33)",
        },
    },
    misc = {
        background_color = "rgba({{colors.background.default.hex_stripped}}FF)",
    },
})

hl.window_rule({
    match        = { pin = 1 },
    border_color = "rgba({{colors.primary.default.hex_stripped}}AA) rgba({{colors.secondary.default.hex_stripped}}77)",
})

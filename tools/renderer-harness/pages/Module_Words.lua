-- Synthetic fixture module authored for this repository (CC0-1.0).
local p = {}

function p.shout(frame)
    local s = frame.args[1] or ''
    return mw.ustring.upper(s) .. ' (' .. mw.ustring.len(s) .. ')'
end

return p

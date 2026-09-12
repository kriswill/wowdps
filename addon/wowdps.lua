-- wowdps: records who is in your raid and which guild each of them belongs
-- to, into this addon's SavedVariables, so the wowdps daemon can associate
-- the players in your combat log with their guilds. The combat log itself
-- never carries a guild name.
--
-- Nothing here is written to disk until logout, /reload or exit — that is
-- when the game flushes SavedVariables — so the daemon learns about a night
-- after it, never during it. Records are keyed by the unit GUID the combat
-- log uses (Player-<realm id>-<hex>), so the join never depends on how a
-- realm name is spelled.
--
-- Installed and kept current by `wowdps addon install`; the daemon rewrites
-- it on start when the copy in the game's AddOns folder is out of date.

local ADDON = ...
local SCHEMA = 1

-- Records older than this are dropped on load: the table is a memory of
-- rosters, not an archive, and a season's worth is plenty.
local KEEP_SECS = 365 * 24 * 60 * 60

-- Looking-for-raid difficulties: never a guild night.
local LFR = { [7] = true, [17] = true }
-- Mythic keystone (8) and mythic (23) dungeons: opt-in (`/wowdps dungeons`).
local KEYSTONE = { [8] = true, [23] = true }

WOWDPS_DATA = WOWDPS_DATA or {}

local function version()
  if C_AddOns and C_AddOns.GetAddOnMetadata then
    return C_AddOns.GetAddOnMetadata(ADDON, "Version")
  end
  return GetAddOnMetadata and GetAddOnMetadata(ADDON, "Version") or nil
end

local function data()
  local d = WOWDPS_DATA
  d.schema = SCHEMA
  d.version = version()
  d.players = d.players or {}
  d.characters = d.characters or {}
  d.config = d.config or { dungeons = false }
  return d
end

local function prune()
  local d = data()
  local cutoff = GetServerTime() - KEEP_SECS
  for guid, rec in pairs(d.players) do
    if type(rec) ~= "table" or (rec.seen or 0) < cutoff then
      d.players[guid] = nil
      d.characters[guid] = nil
    end
  end
end

-- One unit's record and its guid, or nil when the client does not know
-- enough: a unit too far away to answer GetGuildInfo is UNKNOWN, not
-- unguilded, and writing "" for them would overwrite a good record.
local function unitRecord(unit)
  local guid = UnitGUID(unit)
  if not guid or guid:sub(1, 7) ~= "Player-" then
    return nil
  end
  local name, realm = UnitFullName(unit)
  if not name or name == UNKNOWNOBJECT then
    return nil
  end
  realm = realm or GetNormalizedRealmName()
  local guild, rank, _, guildRealm = GetGuildInfo(unit)
  if not guild then
    if unit == "player" then
      -- Guild data lags login by a moment; IsInGuild answers at once.
      if IsInGuild() then
        return nil
      end
    elseif not UnitIsVisible(unit) then
      return nil
    end
  end
  local _, classFile = UnitClass(unit)
  return {
    name = name,
    realm = realm,
    -- "" is a player SEEN without a guild; absent means never written.
    guild = guild or "",
    guild_realm = guild and (guildRealm or realm) or nil,
    rank = rank,
    class = classFile,
    faction = UnitFactionGroup(unit),
    seen = GetServerTime(),
  }, guid
end

-- Where a roster is worth recording: a raid instance off LFR, or (opt-in)
-- a keystone dungeon. Open world, battlegrounds, arenas and PUG-shaped
-- LFR never write a row for anyone but the player.
local function wanted()
  local _, instanceType, difficultyID = GetInstanceInfo()
  if instanceType == "raid" then
    return not LFR[difficultyID]
  elseif instanceType == "party" then
    return data().config.dungeons and KEYSTONE[difficultyID] or false
  end
  return false
end

local function capture()
  local d = data()
  -- The logger's own characters, wherever they are: this is how the daemon
  -- tells "me" from a guildmate when one log alone cannot.
  local rec, guid = unitRecord("player")
  if rec then
    d.players[guid] = rec
    d.characters[guid] = true
  end
  if not wanted() then
    return
  end
  local n = GetNumGroupMembers()
  if n == 0 then
    return
  end
  local prefix, count
  if IsInRaid() then
    prefix, count = "raid", n
  else
    prefix, count = "party", n - 1
  end
  for i = 1, count do
    local r, g = unitRecord(prefix .. i)
    if r then
      d.players[g] = r
    end
  end
end

-- Roster events arrive in bursts; one capture a few seconds after the last
-- of them sees the settled roster.
local pending = false
local function schedule()
  if pending then
    return
  end
  pending = true
  C_Timer.After(5, function()
    pending = false
    capture()
  end)
end

local frame = CreateFrame("Frame")
frame:RegisterEvent("ADDON_LOADED")
frame:RegisterEvent("PLAYER_ENTERING_WORLD")
frame:RegisterEvent("GROUP_ROSTER_UPDATE")
frame:RegisterEvent("PLAYER_GUILD_UPDATE")
frame:RegisterEvent("ZONE_CHANGED_NEW_AREA")
frame:SetScript("OnEvent", function(_, event, arg)
  if event == "ADDON_LOADED" then
    if arg == ADDON then
      prune()
    end
    return
  end
  schedule()
end)

local function count()
  local n, guilded = 0, 0
  for _, rec in pairs(data().players) do
    n = n + 1
    if rec.guild and rec.guild ~= "" then
      guilded = guilded + 1
    end
  end
  return n, guilded
end

SLASH_WOWDPS1 = "/wowdps"
SlashCmdList.WOWDPS = function(msg)
  local d = data()
  msg = (msg or ""):lower():match("^%s*(.-)%s*$")
  if msg == "dungeons" then
    d.config.dungeons = not d.config.dungeons
    print(("wowdps: keystone rosters %s"):format(d.config.dungeons and "recorded" or "ignored"))
  elseif msg == "capture" then
    capture()
    local n, g = count()
    print(("wowdps: captured; %d players known, %d guilded"):format(n, g))
  else
    local n, g = count()
    print(("wowdps %s: %d players known, %d guilded; recording %s; keystone rosters %s"):format(
      d.version or "?", n, g, wanted() and "here" or "not here",
      d.config.dungeons and "on" or "off"))
    print("  /wowdps dungeons — toggle keystone rosters; /wowdps capture — record now")
  end
end

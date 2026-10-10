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
-- It also records how the game classifies each NPC whose nameplate you see
-- inside an instance (normal, elite, rare, rareelite, worldboss, minus, and
-- a lieutenant), keyed by the creature id the combat log's GUIDs carry, so
-- the replay can mark them the way nameplates do. The log has none of it.
--
-- Installed and kept current by `wowdps addon install`; the daemon rewrites
-- it on start when the copy in the game's AddOns folder is out of date.

local ADDON = ...
-- 2: `creatures` joined `players` and `characters`.
local SCHEMA = 2

-- Records older than this are dropped on load: the table is a memory of
-- rosters, not an archive, and a season's worth is plenty.
local KEEP_SECS = 365 * 24 * 60 * 60
-- Creatures unseen this long are dropped on load. The daemon keeps its own
-- copy of every creature it has read, so this only bounds the file.
local CREATURE_KEEP_SECS = 60 * 24 * 60 * 60

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
  d.creatures = d.creatures or {}
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
  cutoff = GetServerTime() - CREATURE_KEEP_SECS
  for id, rec in pairs(d.creatures) do
    if type(rec) ~= "table" or (rec.seen or 0) < cutoff then
      d.creatures[id] = nil
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

-- ---- creatures ---------------------------------------------------------------

-- The client's build as `.build.info` spells it (12.0.5.63906): a creature's
-- classification belongs to the game version that showed it.
local BUILD = (function()
  local v, b = GetBuildInfo()
  return (v and b) and (v .. "." .. b) or v
end)()

-- WOWDPS_DATA is the saved table only from ADDON_LOADED on; a write before
-- it would land in the placeholder and be lost.
local loaded = false

-- A 12.x secret value (combat-restricted data) must never be compared or
-- stored: either errors.
local function secret(v)
  return issecretvalue ~= nil and issecretvalue(v) or false
end

-- Where a creature is worth recording: inside a dungeon, a raid or a
-- scenario (a delve), where the pulls a replay keeps happen; outdoors only
-- a world boss. Questing would fill the table with mobs no replay shows.
local function creatureWanted(classification)
  local inInstance, instanceType = IsInInstance()
  if inInstance and (instanceType == "party" or instanceType == "raid"
      or instanceType == "scenario") then
    return true
  end
  return classification == "worldboss"
end

-- The group's names, rebuilt after a roster change. Some encounters spawn
-- NPCs that wear a group member's name (a mirror clone); the table holds
-- NPC names only, so a creature named like someone in the group is skipped.
local rosterNames, rosterDirty = {}, true
local function groupNamed(name)
  if rosterDirty then
    rosterDirty = false
    rosterNames = {}
    local units = { "player" }
    local prefix = IsInRaid() and "raid" or "party"
    for i = 1, GetNumGroupMembers() do
      units[#units + 1] = prefix .. i
    end
    for _, u in ipairs(units) do
      local member = UnitName(u)
      if member and not secret(member) then
        rosterNames[member] = true
      end
    end
  end
  return rosterNames[name] == true
end

-- Record one unit's classification under its creature id, or nothing: a
-- player, a pet, a guardian or anything else a player controls, a unit the
-- client cannot name yet, one named like a group member, a value the game
-- keeps secret.
local function recordCreature(unit)
  if not loaded or not unit or not UnitExists(unit) then
    return
  end
  local guid = UnitGUID(unit)
  if secret(guid) or not guid then
    return
  end
  -- Creature-0-<server>-<instance>-<zone uid>-<creature id>-<spawn uid>,
  -- the layout the combat log writes; a vehicle NPC shares it.
  local kind, id = guid:match("^(%a+)%-%d+%-%d+%-%d+%-%d+%-(%d+)%-")
  if kind ~= "Creature" and kind ~= "Vehicle" then
    return
  end
  local controlled = UnitPlayerControlled(unit)
  if secret(controlled) or controlled then
    return
  end
  local classification = UnitClassification(unit)
  if secret(classification) or not classification or not creatureWanted(classification) then
    return
  end
  local name = UnitName(unit)
  if secret(name) or not name or name == UNKNOWNOBJECT or groupNamed(name) then
    return
  end
  local lieutenant = 0
  if UnitIsLieutenant then -- new in 12.x; older clients have none
    local yes = UnitIsLieutenant(unit)
    if not secret(yes) and yes then
      lieutenant = 1
    end
  end
  local _, _, difficulty = GetInstanceInfo()
  local creatureType = UnitCreatureType(unit)
  local _, power = UnitPowerType(unit)
  local creatures = WOWDPS_DATA.creatures
  id = tonumber(id)
  local rec = creatures[id]
  if not rec then
    rec = {}
    creatures[id] = rec
  end
  -- The newest sighting wins whole: a creature id's classification can
  -- differ by difficulty, and the difficulty it was seen at rides along.
  rec.name = name
  rec.classification = classification
  rec.lieutenant = lieutenant
  rec.seen = GetServerTime()
  rec.build = BUILD
  rec.difficulty = difficulty
  rec.type = not secret(creatureType) and creatureType or nil
  rec.power = not secret(power) and power or nil
end

local frame = CreateFrame("Frame")
frame:RegisterEvent("ADDON_LOADED")
frame:RegisterEvent("PLAYER_ENTERING_WORLD")
frame:RegisterEvent("GROUP_ROSTER_UPDATE")
frame:RegisterEvent("PLAYER_GUILD_UPDATE")
frame:RegisterEvent("ZONE_CHANGED_NEW_AREA")
frame:RegisterEvent("NAME_PLATE_UNIT_ADDED")
frame:RegisterEvent("UNIT_CLASSIFICATION_CHANGED")
frame:RegisterEvent("PLAYER_TARGET_CHANGED")
frame:SetScript("OnEvent", function(_, event, arg)
  if event == "ADDON_LOADED" then
    if arg == ADDON then
      prune()
      loaded = true
    end
    return
  elseif event == "NAME_PLATE_UNIT_ADDED" or event == "UNIT_CLASSIFICATION_CHANGED" then
    -- Mid-pull, on every nameplate: an API this client answers oddly must
    -- never put an error on the player's screen.
    pcall(recordCreature, arg)
    return
  elseif event == "PLAYER_TARGET_CHANGED" then
    pcall(recordCreature, "target")
    return
  end
  rosterDirty = true
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

local function countCreatures()
  local n, lieutenants = 0, 0
  for _, rec in pairs(data().creatures) do
    n = n + 1
    if rec.lieutenant == 1 then
      lieutenants = lieutenants + 1
    end
  end
  return n, lieutenants
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
    local c, lt = countCreatures()
    print(("wowdps %s: %d players known, %d guilded; recording %s; keystone rosters %s"):format(
      d.version or "?", n, g, wanted() and "here" or "not here",
      d.config.dungeons and "on" or "off"))
    print(("  %d creatures classified, %d lieutenants"):format(c, lt))
    print("  /wowdps dungeons — toggle keystone rosters; /wowdps capture — record now")
  end
end

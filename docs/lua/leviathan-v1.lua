---@meta

-- Git Leviathan Lua API annotations.
-- Compatibility surface: api_version = "1.0".

---@alias LeviathanLength number|'"fill"'|'"shrink"'
---@alias LeviathanColor string
---@alias LeviathanJson nil|boolean|number|string|table
---@alias LeviathanWidget LeviathanTextWidget|LeviathanButtonWidget|LeviathanRowWidget|LeviathanColumnWidget|LeviathanContainerWidget|LeviathanPaddingWidget|LeviathanSpaceWidget|LeviathanIconWidget|LeviathanImageWidget|LeviathanScrollableWidget|LeviathanMouseAreaWidget|LeviathanTablistWidget|LeviathanResizableSplitWidget

---@class Leviathan
---@field api LeviathanApi
---@field ui LeviathanUi
---@field fs LeviathanFs
---@field env LeviathanEnv
---@field repository LeviathanRepository
---@field tab_registry LeviathanTabRegistry
---@field services LeviathanServices
---@field persist LeviathanPersist
---@field health LeviathanHealth
leviathan = {}

---@param message string
function leviathan.log(message) end

---@class LeviathanApi
leviathan.api = {}

---@class LeviathanAutocmdOptions
---@field callback fun(event: string)

---@param events string[]
---@param opts LeviathanAutocmdOptions
function leviathan.api.create_autocmd(events, opts) end

---@param callback fun()
function leviathan.api.schedule(callback) end

---@param ms integer
---@param callback fun()
function leviathan.api.defer_fn(ms, callback) end

---@param name string
---@param callback fun()
function leviathan.api.create_user_command(name, callback) end

---@class LeviathanUi
---@field main_bar LeviathanRegionHandle
---@field tab_bar LeviathanRegionHandle
---@field repository LeviathanRegionHandle
leviathan.ui = {}

---@return string[]
function leviathan.ui.list_regions() end

---@param name string
---@return LeviathanRegionHandle
function leviathan.ui.region(name) end

---@class LeviathanScreenSpec
---@field id string
---@field init fun(): table
---@field view fun(state: table): LeviathanWidget
---@field update fun(state: table, event: string, value: LeviathanJson): table
---@field serialize? fun(state: table): table
---@field deserialize? fun(data: table): table

---@param spec LeviathanScreenSpec
function leviathan.ui.register_screen(spec) end

---@class LeviathanRegionHandle
local LeviathanRegionHandle = {}

---@class LeviathanSlotSpec
---@field id string
---@field section string
---@field pane? string
---@field priority integer
---@field widget LeviathanWidget|fun(): LeviathanWidget
---@field on_click? fun(slot_id: string, event: string, value: LeviathanJson): table|nil

---@class LeviathanSlotTarget
---@field id string
---@field section string
---@field pane? string

---@param spec LeviathanSlotSpec
function LeviathanRegionHandle.add(spec) end

---@param target LeviathanSlotTarget
function LeviathanRegionHandle.remove(target) end

---@param target LeviathanSlotTarget
---@param spec LeviathanSlotSpec
function LeviathanRegionHandle.replace(target, spec) end

---@class LeviathanFsEntry
---@field name string
---@field path string
---@field is_dir boolean
---@field is_symlink boolean
---@field size integer
---@field modified integer

---@class LeviathanFs
leviathan.fs = {}

---@param path string
---@return string|nil content
---@return string|nil err
function leviathan.fs.read_file(path) end

---@param path string
---@return string[]|nil lines
---@return string|nil err
function leviathan.fs.read_lines(path) end

---@param path string
---@param content string
---@return boolean ok
---@return string|nil err
function leviathan.fs.write_file(path, content) end

---@param path string
---@param content string
---@return boolean ok
---@return string|nil err
function leviathan.fs.append_file(path, content) end

---@param path string
---@return boolean ok
---@return string|nil err
function leviathan.fs.delete(path) end

---@param path string
---@return boolean ok
---@return string|nil err
function leviathan.fs.mkdir(path) end

---@param src string
---@param dst string
---@return boolean ok
---@return string|nil err
function leviathan.fs.copy(src, dst) end

---@param src string
---@param dst string
---@return boolean ok
---@return string|nil err
function leviathan.fs.rename(src, dst) end

---@param path string
---@return boolean ok
---@return string|nil err
function leviathan.fs.touch(path) end

---@param path string
---@return string|nil target
---@return string|nil err
function leviathan.fs.read_link(path) end

---@param path string
---@return LeviathanFsEntry[]|nil entries
---@return string|nil err
function leviathan.fs.list_dir(path) end

---@param path string
---@return boolean
function leviathan.fs.exists(path) end

---@param path string
---@return boolean
function leviathan.fs.is_file(path) end

---@param path string
---@return boolean
function leviathan.fs.is_dir(path) end

---@param path string
---@return boolean
function leviathan.fs.is_symlink(path) end

---@param path string
---@return integer|nil bytes
---@return string|nil err
function leviathan.fs.size(path) end

---@param path string
---@return integer|nil unix_seconds
---@return string|nil err
function leviathan.fs.modified(path) end

---@param path string
---@return LeviathanFsEntry|nil metadata
---@return string|nil err
function leviathan.fs.metadata(path) end

---@param path string
---@return boolean
function leviathan.fs.is_absolute(path) end

---@param path string
---@return string|nil
function leviathan.fs.parent(path) end

---@param path string
---@return string|nil
function leviathan.fs.basename(path) end

---@param path string
---@return string|nil
function leviathan.fs.stem(path) end

---@param path string
---@return string|nil
function leviathan.fs.extension(path) end

---@param a string
---@param b string
---@return string
function leviathan.fs.join(a, b) end

---@param path string
---@param base string
---@return string|nil
function leviathan.fs.relative_to(path, base) end

---@param path string
---@param ext string
---@return string|nil
function leviathan.fs.with_extension(path, ext) end

---@param path string
---@param name string
---@return string|nil
function leviathan.fs.with_file_name(path, name) end

---@return string|nil path
---@return string|nil err
function leviathan.fs.cwd() end

---@return string|nil
function leviathan.fs.home() end

---@return string
function leviathan.fs.temp_dir() end

---@return string|nil
function leviathan.fs.config_dir() end

---@return string|nil
function leviathan.fs.cache_dir() end

---@return string|nil
function leviathan.fs.data_dir() end

---@return string|nil
function leviathan.fs.state_dir() end

---@param path string
---@return string|nil path
---@return string|nil err
function leviathan.fs.canonicalize(path) end

---@class LeviathanEnv
leviathan.env = {}

---@param name string
---@return string|nil value
---@return string|nil err
function leviathan.env.get(name) end

---@return table<string, string>
function leviathan.env.list() end

---@class LeviathanRepositoryRemoteBranch
---@field name string
---@field remote_name string
---@field hash string

---@class LeviathanRepositoryLocalBranch
---@field name string
---@field hash string
---@field is_current boolean
---@field upstream_branch LeviathanRepositoryRemoteBranch|nil

---@class LeviathanRepositoryTag
---@field name string
---@field hash string

---@class LeviathanRepository
---@field name string
---@field workdir_path string
---@field current_branch_name string
---@field current_branch LeviathanRepositoryLocalBranch|nil
---@field is_open boolean
---@field is_detached boolean
---@field is_unborn boolean
---@field is_bare boolean
---@field head_hash string
---@field default_remote_name string
---@field local_branches LeviathanRepositoryLocalBranch[]
---@field remote_branches LeviathanRepositoryRemoteBranch[]
---@field tags LeviathanRepositoryTag[]
leviathan.repository = {}

---@class LeviathanTab
---@field path string
---@field name string

---@class LeviathanTabRegistry
---@field list LeviathanTab[]
---@field current LeviathanTab|nil
leviathan.tab_registry = {}

---@param path string
function leviathan.tab_registry.add(path) end

---@param path string
function leviathan.tab_registry.remove(path) end

---@param path string
function leviathan.tab_registry.select(path) end

---@param paths string[]
function leviathan.tab_registry.reorder(paths) end

---@class LeviathanServices
leviathan.services = {}

---@param name_at_version string
---@param methods table<string, fun(...): LeviathanJson>
function leviathan.services.register(name_at_version, methods) end

---@param name_at_version string
---@return table<string, fun(...): LeviathanJson>
function leviathan.services.get(name_at_version) end

---@class LeviathanPersistOpenOptions
---@field version? integer
---@field migrations? LeviathanPersistMigration[]

---@class LeviathanPersistMigration
---@field from integer
---@field to integer
---@field transform fun(old: LeviathanJson): LeviathanJson

---@class LeviathanPersistStore
local LeviathanPersistStore = {}

---@param key string
---@return LeviathanJson
function LeviathanPersistStore:get(key) end

---@param key string
---@param value LeviathanJson
function LeviathanPersistStore:set(key, value) end

---@return integer
function LeviathanPersistStore:version() end

---@class LeviathanPersist
leviathan.persist = {}

---@param name string
---@param opts? LeviathanPersistOpenOptions
---@return LeviathanPersistStore
function leviathan.persist.open(name, opts) end

---@class LeviathanHealthContext
local LeviathanHealthContext = {}

---@param message string
function LeviathanHealthContext:ok(message) end

---@param message string
function LeviathanHealthContext:info(message) end

---@param message string
function LeviathanHealthContext:warn(message) end

---@param message string
function LeviathanHealthContext:error(message) end

---@class LeviathanHealth
leviathan.health = {}

---@param callback fun(ctx: LeviathanHealthContext)
function leviathan.health.register(callback) end

---@class LeviathanTextWidget
---@field kind '"text"'
---@field value? string
---@field size? number
---@field color? LeviathanColor

---@class LeviathanButtonStyleBorder
---@field width? number
---@field radius? number
---@field color? LeviathanColor

---@class LeviathanButtonStyle
---@field background? LeviathanColor
---@field background_hover? LeviathanColor
---@field text_color? LeviathanColor
---@field border? LeviathanButtonStyleBorder

---@class LeviathanButtonWidget
---@field kind '"button"'
---@field child? LeviathanWidget
---@field text? string
---@field on_click? string
---@field value? LeviathanJson
---@field width? LeviathanLength
---@field height? LeviathanLength
---@field style? LeviathanButtonStyle

---@class LeviathanRowWidget
---@field kind '"row"'
---@field children? LeviathanWidget[]
---@field spacing? number
---@field width? LeviathanLength
---@field height? LeviathanLength
---@field align_y? string

---@class LeviathanColumnWidget
---@field kind '"column"'
---@field children? LeviathanWidget[]
---@field spacing? number
---@field width? LeviathanLength
---@field height? LeviathanLength
---@field align_x? string

---@class LeviathanContainerWidget
---@field kind '"container"'
---@field child? LeviathanWidget
---@field bg? LeviathanColor
---@field width? LeviathanLength
---@field height? LeviathanLength
---@field max_width? number
---@field max_height? number
---@field min_width? number
---@field min_height? number
---@field center_x? boolean
---@field center_y? boolean

---@class LeviathanPaddingWidget
---@field kind '"padding"'
---@field top? number
---@field right? number
---@field bottom? number
---@field left? number
---@field width? LeviathanLength
---@field height? LeviathanLength
---@field child? LeviathanWidget

---@class LeviathanSpaceWidget
---@field kind '"space"'
---@field width? LeviathanLength
---@field height? LeviathanLength

---@class LeviathanIconWidget
---@field kind '"icon"'
---@field path? string
---@field size? number
---@field color? LeviathanColor

---@class LeviathanImageWidget
---@field kind '"image"'
---@field path? string
---@field size? number

---@class LeviathanScrollableWidget
---@field kind '"scrollable"'
---@field child? LeviathanWidget
---@field width? LeviathanLength
---@field height? LeviathanLength

---@class LeviathanMouseAreaWidget
---@field kind '"mouse_area"'
---@field child? LeviathanWidget
---@field on_click? string
---@field value? LeviathanJson

---@class LeviathanTablistTab
---@field id LeviathanJson
---@field name? string

---@class LeviathanTablistWidget
---@field kind '"tablist"'
---@field tabs? LeviathanTablistTab[]
---@field active? LeviathanJson
---@field orderable? boolean
---@field on_select? string
---@field on_close? string
---@field on_reorder? string

---@class LeviathanResizableSplitWidget
---@field kind '"resizable_split"'
---@field id? string
---@field direction? '"horizontal"'|'"vertical"'
---@field children? LeviathanWidget[]

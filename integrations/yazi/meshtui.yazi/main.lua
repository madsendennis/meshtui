--- @since 25.5.31
--- meshtui previewer for yazi: renders a mesh file (PLY/STL/OBJ/DRC/GLB) to a
--- cached PNG via `meshtui --screenshot`, then shows it like an image.
---
--- Folders are NOT handled here — yazi's built-in `folder` previewer lists
--- them. To open a mesh file or a whole folder in meshtui, bind the `mesh`
--- opener to a key (see README); the plugin only previews single mesh files.
---
--- ~/.config/yazi/yazi.toml:
---   [plugin]
---   prepend_previewers = [
---     { url = "*.{ply,stl,obj,drc,glb}", run = "meshtui" },
---   ]
---   prepend_preloaders = [
---     { url = "*.{ply,stl,obj,drc,glb}", run = "meshtui" },
---   ]

local M = {}

-- Render the mesh file to `cache` (PNG), sized to the preview area.
local function render(job, cache)
	if fs.cha(cache) then
		return true -- already rendered & cached
	end

	local output, err = Command("meshtui")
		:arg({
			tostring(job.file.path),
			"--screenshot",
			tostring(cache),
			"--size",
			string.format("%dx%d", rt.preview.max_width, rt.preview.max_height),
		})
		:output()

	if not output then
		return true, Err("Failed to start `meshtui`, error: %s", err)
	elseif not output.status.success then
		return true, Err("meshtui: %s", (output.stderr or ""):gsub("%s+$", ""))
	end

	return ya.image_precache(Url(cache), cache)
end

function M:peek(job)
	local start, cache = os.clock(), ya.file_cache(job)
	if not cache then
		return
	end

	local ok, err = self:preload(job)
	if not ok or err then
		return ya.preview_widget(job, err)
	end

	ya.sleep(math.max(0, rt.preview.image_delay / 1000 + start - os.clock()))

	local _, err = ya.image_show(cache, job.area)
	ya.preview_widget(job, err)
end

function M:seek() end

function M:preload(job)
	local cache = ya.file_cache(job)
	if not cache then
		return true
	end
	return render(job, cache)
end

return M

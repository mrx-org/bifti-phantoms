# The public phantom catalog and registry (../../REGISTRY.md): list what is
# discoverable, resolve a label to an immutable collection name, and download a
# phantom's JSON plus every NIfTI it references from Zenodo into a local cache.

const REGISTRY_URL = "https://raw.githubusercontent.com/mrx-org/bifti-phantoms/refs/heads/main/registry.json"
const CATALOG_URL = "https://raw.githubusercontent.com/mrx-org/bifti-phantoms/refs/heads/main/catalog.json"
# A Zenodo version DOI ("10.5281/zenodo.<id>") embeds the record id, which is all
# that is needed to fetch a file from the API.
zenodo_file_url(record_id, filename) = "https://zenodo.org/api/records/$record_id/files/$(escape_uri(filename))/content"

function zenodo_record_id(doi)
    m = match(r"zenodo\.(\d+)$", doi)
    isnothing(m) && throw(ArgumentError("Not a Zenodo DOI: $(repr(doi))"))
    return m[1]
end

const URI_UNRESERVED = codeunits("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")
escape_uri(s) = join(b in URI_UNRESERVED ? string(Char(b)) : "%" * uppercase(string(b; base=16, pad=2)) for b in codeunits(s))

fetch_json(url) = JSON.parse(String(take!(Downloads.download(url, IOBuffer()))))

"""
    load_catalog(url=CATALOG_URL) -> AbstractDict{String,String}

Download catalog.json, the living discovery list: it maps a human-readable label
to an immutable registry collection name. Look each value up in [`load_registry`](@ref).
"""
load_catalog(url=CATALOG_URL) = fetch_json(url)

"""
    load_registry(url=REGISTRY_URL) -> AbstractDict

Download registry.json, the immutable archive of every published collection, keyed
by its permanent `<author>-<name>-<number>` name. Each entry holds `description`,
`authors`, `license`, `doi` and `phantoms`; see [`flatten_phantoms`](@ref).
"""
load_registry(url=REGISTRY_URL) = fetch_json(url)

"""
    flatten_phantoms(phantoms) -> Vector{String}

Every phantom filename in a collection's (possibly nested) `phantoms` list,
depth-first. Entries are filenames or `{"group": ..., "phantoms": [...]}` objects.
Also accepts a whole registry entry.
"""
flatten_phantoms(entry::AbstractDict) = flatten_phantoms(entry["phantoms"])
flatten_phantoms(phantoms::AbstractVector) =
    String[file for entry in phantoms for file in (entry isa AbstractString ? (entry,) : flatten_phantoms(entry))]

default_cache_dir() = @get_scratch!("phantoms")

"""
    load_registry_phantom(collection, name; registry=load_registry(), cache_dir) -> String

Download the phantom JSON `name` of the registry `collection` (an immutable registry
name, i.e. a catalog value, not a label) and every NIfTI it references into
`cache_dir`, then return the JSON path - ready for [`load_bifti`](@ref). Files that
are already cached are not downloaded again: a DOI always resolves to the same bytes.
The default `cache_dir` is a scratch space of this package.
"""
function load_registry_phantom(collection, name; registry=load_registry(), cache_dir=default_cache_dir())
    haskey(registry, collection) || throw(KeyError(collection))
    doi = registry[collection]["doi"]
    record_id = zenodo_record_id(doi)
    dir = mkpath(joinpath(cache_dir, "$collection-$(replace(doi, "/" => "_"))"))

    json_path = download_json(dir, record_id, name)
    for file in nifti_files(read_bifti(json_path))
        # NIfTIs are stored next to the JSON (../../JSON.md) and Zenodo records are flat,
        # so a path with directories could not be downloaded to where the loader reads it.
        basename(file) == file || throw(ArgumentError("NIfTI reference $(repr(file)) must be a plain filename next to the phantom JSON"))
        dest = joinpath(dir, file)
        isfile(dest) || download_atomic(zenodo_file_url(record_id, file), dest)
    end
    return json_path
end

# Download to a temporary file first, so an interrupted transfer never leaves a
# truncated file that later runs would mistake for a cached one.
function download_atomic(url, dest)
    tmp = tempname(dirname(dest))
    try
        Downloads.download(url, tmp)
    catch
        rm(tmp; force=true)
        rethrow()
    end
    return mv(tmp, dest; force=true)
end

# Resolve a phantom JSON in the spec's lookup order (../../REGISTRY.md): fetch `name`
# directly from the record, else extract it from the record's `configs.tar`.
function download_json(dir, record_id, name)
    dest = joinpath(dir, name)
    isfile(dest) && return dest
    try
        return download_atomic(zenodo_file_url(record_id, name), dest)
    catch err
        err isa Downloads.RequestError || rethrow()
    end
    archive = joinpath(dir, "configs.tar")
    isfile(archive) || download_atomic(zenodo_file_url(record_id, "configs.tar"), archive)
    return mktempdir() do extracted
        Tar.extract(header -> header.path == name, archive, extracted)
        isfile(joinpath(extracted, name)) ||
            throw(ArgumentError("$(repr(name)) is neither a file of Zenodo record $record_id nor in its configs.tar"))
        mkpath(dirname(dest))
        mv(joinpath(extracted, name), dest; force=true)
    end
end

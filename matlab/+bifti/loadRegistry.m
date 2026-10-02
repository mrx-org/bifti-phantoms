function registry = loadRegistry(source)
%LOADREGISTRY The public BIfTI registry of every published collection.
%   registry = bifti.loadRegistry() downloads registry.json, the immutable
%   archive keyed by <author>-<name>-<number> (REGISTRY.md). It is a struct
%   with the fields 'keys' (collection names) and 'values' (entries with
%   description, authors, license, doi and phantoms, as JSON objects); read
%   an entry with bifti.internal.jsonGet or list its phantoms with
%   bifti.flattenPhantoms. Pass a URL or a local file path as SOURCE to read
%   another copy.
%
%   See also bifti.loadCatalog, bifti.flattenPhantoms, bifti.loadRegistryPhantom.
    if nargin < 1
        source = 'https://raw.githubusercontent.com/mrx-org/bifti-phantoms/refs/heads/main/registry.json';
    end
    registry = bifti.internal.parseJson(bifti.internal.fetchText(source));
end

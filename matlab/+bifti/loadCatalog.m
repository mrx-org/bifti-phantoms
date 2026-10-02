function catalog = loadCatalog(source)
%LOADCATALOG The public BIfTI catalog: label -> immutable registry name.
%   catalog = bifti.loadCatalog() downloads catalog.json, the living
%   discovery list (REGISTRY.md), as a struct with the fields 'keys' (labels,
%   in file order) and 'values' (registry collection names). Pass a URL or a
%   local file path as SOURCE to read another copy.
%
%   See also bifti.loadRegistry, bifti.loadRegistryPhantom.
    if nargin < 1
        source = 'https://raw.githubusercontent.com/mrx-org/bifti-phantoms/refs/heads/main/catalog.json';
    end
    catalog = bifti.internal.parseJson(bifti.internal.fetchText(source));
end

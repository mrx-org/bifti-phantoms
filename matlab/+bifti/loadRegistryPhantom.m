function path = loadRegistryPhantom(collection, name, varargin)
%LOADREGISTRYPHANTOM Download a phantom of the public registry into a cache.
%   path = bifti.loadRegistryPhantom(collection, name) downloads the phantom
%   JSON NAME of the registry COLLECTION (an immutable registry name, i.e. a
%   catalog value, not a label) and every NIfTI it references, and returns
%   the JSON path, ready for bifti.loadPhantom. Cached files are not
%   downloaded again: a Zenodo version DOI always resolves to the same bytes.
%
%   Name-value options:
%     'Registry'   registry from bifti.loadRegistry (default: download it)
%     'CacheDir'   cache folder (default: ~/.cache/bifti)
%     'ZenodoApi'  Zenodo records API (default: https://zenodo.org/api/records)
%
%   See also bifti.loadCatalog, bifti.loadRegistry, bifti.loadPhantom.
    options = inputParser;
    options.addParameter('Registry', []);
    options.addParameter('CacheDir', defaultCacheDir());
    options.addParameter('ZenodoApi', 'https://zenodo.org/api/records');
    options.parse(varargin{:});
    registry = options.Results.Registry;
    if isempty(registry)
        registry = bifti.loadRegistry();
    end

    entry = bifti.internal.jsonGet(registry, collection, []);
    if isempty(entry)
        error('bifti:registry', 'Collection "%s" is not in the registry', collection);
    end
    doi = bifti.internal.jsonGet(entry, 'doi');
    recordId = regexp(doi, 'zenodo\.(\d+)$', 'tokens', 'once');
    if isempty(recordId)
        error('bifti:registry', 'Not a Zenodo DOI: "%s"', doi);
    end
    fileUrl = @(file) sprintf('%s/%s/files/%s/content', options.Results.ZenodoApi, recordId{1}, escapeUri(file));
    folder = fullfile(options.Results.CacheDir, [collection '-' strrep(doi, '/', '_')]);
    if ~exist(folder, 'dir')
        mkdir(folder);
    end

    path = downloadJson(folder, fileUrl, name, recordId{1});
    for file = bifti.niftiFiles(bifti.readPhantom(path))
        [~, base, ext] = fileparts(file{1});
        dest = fullfile(folder, [base ext]);
        if ~exist(dest, 'file')
            bifti.internal.download(fileUrl([base ext]), dest);
        end
    end
end

% Resolve a phantom JSON in the lookup order of REGISTRY.md: the file itself,
% else the record's configs.tar.
function dest = downloadJson(folder, fileUrl, name, recordId)
    dest = fullfile(folder, name);
    if exist(dest, 'file')
        return
    end
    try
        bifti.internal.download(fileUrl(name), dest);
        return
    catch
    end
    archive = fullfile(folder, 'configs.tar');
    if ~exist(archive, 'file')
        bifti.internal.download(fileUrl('configs.tar'), archive);
    end
    extracted = tempname;
    mkdir(extracted);
    cleanup = onCleanup(@() rmdir(extracted, 's'));
    untar(archive, extracted);
    if ~exist(fullfile(extracted, name), 'file')
        error('bifti:registry', '"%s" is neither a file of Zenodo record %s nor in its configs.tar', name, recordId);
    end
    copyfile(fullfile(extracted, name), dest);
end

function folder = defaultCacheDir()
    home = getenv('HOME');
    if isempty(home)
        home = getenv('USERPROFILE');
    end
    folder = fullfile(home, '.cache', 'bifti');
end

function s = escapeUri(s)
    bytes = unicode2native(s, 'UTF-8');
    unreserved = ismember(char(bytes), ['A':'Z', 'a':'z', '0':'9', '-._~']);
    parts = cellstr(char(bytes(:)));
    parts(~unreserved) = cellstr(num2str(double(bytes(~unreserved))', '%%%02X'));
    s = [parts{:}];
end

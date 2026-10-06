function files = flattenPhantoms(phantoms)
%FLATTENPHANTOMS Every phantom JSON name of a registry collection, depth-first.
%   files = bifti.flattenPhantoms(entry) for a registry entry, or for its
%   'phantoms' list, whose items are file names or groups
%   {"group": ..., "phantoms": [...]} nested to any depth.
    if isstruct(phantoms)
        phantoms = bifti.internal.jsonGet(phantoms, 'phantoms');
    end
    files = {};
    for k = 1:numel(phantoms)
        if ischar(phantoms{k})
            files{end + 1} = phantoms{k}; %#ok<AGROW>
        else
            files = [files, bifti.flattenPhantoms(phantoms{k})]; %#ok<AGROW>
        end
    end
end

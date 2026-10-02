function download(url, dest)
%DOWNLOAD Save URL to DEST, atomically: an interrupted transfer never leaves a
%   truncated file that a later call would mistake for a cached one.
%   file:// URLs are copied, which keeps tests and local mirrors offline.
    tmp = [dest '.part'];
    try
        if strncmp(url, 'file://', 7)
            [ok, message] = copyfile(urlToPath(url), tmp);
            if ~ok
                error('bifti:download', '%s', message);
            end
        elseif exist('OCTAVE_VERSION', 'builtin')
            urlwrite(url, tmp);  % Octave has no websave
        else
            websave(tmp, url, weboptions('Timeout', 60));
        end
    catch err
        if exist(tmp, 'file')
            delete(tmp);
        end
        error('bifti:download', 'Downloading %s failed: %s', url, err.message);
    end
    movefile(tmp, dest, 'f');
end

function path = urlToPath(url)
    path = url(8:end);
    % Undo percent-encoding.
    [tokens, parts] = regexp(path, '%([0-9A-Fa-f]{2})', 'tokens', 'split');
    path = parts{1};
    for k = 1:numel(tokens)
        path = [path, char(hex2dec(tokens{k}{1})), parts{k + 1}]; %#ok<AGROW>
    end
end

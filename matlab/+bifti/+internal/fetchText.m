function text = fetchText(source)
%FETCHTEXT Text of a local file, a file:// URL or an http(s) URL.
    if exist(source, 'file')
        text = fileread(source);
    else
        tmp = [tempname '.txt'];
        cleanup = onCleanup(@() deleteIfExists(tmp));
        bifti.internal.download(source, tmp);
        text = fileread(tmp);
    end
end

function deleteIfExists(path)
    if exist(path, 'file')
        delete(path);
    end
end

function text = writeJson(value, indent)
%WRITEJSON Serialize a value to pretty-printed JSON.
%   Objects are structs with 'keys' and 'values' (see parseJson), arrays are
%   cell arrays or numeric vectors, strings char vectors.
    if nargin < 2
        indent = '';
    end
    inner = [indent '  '];
    if isstruct(value)
        if isempty(value.keys)
            text = '{}';
            return
        end
        members = cellfun(@(k, v) [inner quote(k) ': ' bifti.internal.writeJson(v, inner)], ...
            value.keys, value.values, 'UniformOutput', false);
        text = ['{' char(10) strjoin(members, [',' char(10)]) char(10) indent '}'];
    elseif iscell(value)
        if isempty(value)
            text = '[]';
            return
        end
        items = cellfun(@(v) [inner bifti.internal.writeJson(v, inner)], value, 'UniformOutput', false);
        text = ['[' char(10) strjoin(items, [',' char(10)]) char(10) indent ']'];
    elseif ischar(value)
        text = quote(value);
    elseif islogical(value)
        text = lower(mat2str(value));
    elseif isnumeric(value) && isscalar(value)
        if ~isfinite(value)
            error('bifti:json', 'JSON cannot represent %g', value);
        end
        text = sprintf('%.17g', value);
    elseif isnumeric(value)
        text = bifti.internal.writeJson(num2cell(value), indent);
    else
        error('bifti:json', 'Cannot serialize a value of class %s', class(value));
    end
end

function text = quote(s)
    s = strrep(s, '\', '\\');
    s = strrep(s, '"', '\"');
    s = strrep(s, char(10), '\n');
    s = strrep(s, char(9), '\t');
    text = ['"' s '"'];
end

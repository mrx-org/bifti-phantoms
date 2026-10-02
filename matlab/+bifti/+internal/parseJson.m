function value = parseJson(text)
%PARSEJSON Parse JSON text, keeping object keys verbatim and in order.
%   MATLAB's jsondecode turns keys into valid field names, which mangles the
%   BIfTI keys "T2'", "B1+" and "B1-" (the latter two even collide) as well as
%   arbitrary tissue names. Here objects become structs with the fields
%   'keys' and 'values' (cell arrays, see bifti.internal.jsonGet), arrays
%   become cell arrays, numbers doubles, booleans logicals and null [].
    text = char(text);
    [value, pos] = parseValue(text, skipSpace(text, 1));
    pos = skipSpace(text, pos);
    if pos <= numel(text)
        error('bifti:json', 'Unexpected trailing content at position %d', pos);
    end
end

function [value, pos] = parseValue(text, pos)
    if pos > numel(text)
        error('bifti:json', 'Unexpected end of JSON');
    end
    switch text(pos)
        case '{'
            [value, pos] = parseObject(text, pos);
        case '['
            [value, pos] = parseArray(text, pos);
        case '"'
            [value, pos] = parseString(text, pos);
        otherwise
            [value, pos] = parseLiteral(text, pos);
    end
end

function [object, pos] = parseObject(text, pos)
    keys = {};
    values = {};
    pos = skipSpace(text, pos + 1);
    if text(pos) == '}'
        object = struct('keys', {keys}, 'values', {values});
        pos = pos + 1;
        return
    end
    while true
        [key, pos] = parseString(text, skipSpace(text, pos));
        pos = expect(text, skipSpace(text, pos), ':');
        [value, pos] = parseValue(text, skipSpace(text, pos));
        keys{end + 1} = key; %#ok<AGROW>
        values{end + 1} = value; %#ok<AGROW>
        pos = skipSpace(text, pos);
        if text(pos) == '}'
            break
        end
        pos = expect(text, pos, ',');
    end
    object = struct('keys', {keys}, 'values', {values});
    pos = pos + 1;
end

function [array, pos] = parseArray(text, pos)
    array = {};
    pos = skipSpace(text, pos + 1);
    if text(pos) == ']'
        pos = pos + 1;
        return
    end
    while true
        [array{end + 1}, pos] = parseValue(text, skipSpace(text, pos)); %#ok<AGROW>
        pos = skipSpace(text, pos);
        if text(pos) == ']'
            break
        end
        pos = expect(text, pos, ',');
    end
    pos = pos + 1;
end

function [value, pos] = parseString(text, pos)
    if text(pos) ~= '"'
        error('bifti:json', 'Expected a string at position %d', pos);
    end
    pos = pos + 1;
    value = '';
    while text(pos) ~= '"'
        if text(pos) == '\'
            switch text(pos + 1)
                case 'b', c = char(8);
                case 'f', c = char(12);
                case 'n', c = char(10);
                case 'r', c = char(13);
                case 't', c = char(9);
                case 'u'
                    c = char(hex2dec(text(pos + 2:pos + 5)));
                    pos = pos + 4;
                otherwise, c = text(pos + 1);
            end
            value(end + 1) = c; %#ok<AGROW>
            pos = pos + 2;
        else
            value(end + 1) = text(pos); %#ok<AGROW>
            pos = pos + 1;
        end
    end
    pos = pos + 1;
end

function [value, pos] = parseLiteral(text, pos)
    literals = {'true', 'false', 'null'};
    literalValues = {true, false, []};
    for k = 1:numel(literals)
        n = numel(literals{k});
        if pos + n - 1 <= numel(text) && strcmp(text(pos:pos + n - 1), literals{k})
            value = literalValues{k};
            pos = pos + n;
            return
        end
    end
    number = regexp(text(pos:min(end, pos + 63)), '^-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?', 'match', 'once');
    if isempty(number)
        error('bifti:json', 'Unexpected character ''%s'' at position %d', text(pos), pos);
    end
    value = str2double(number);
    pos = pos + numel(number);
end

function pos = skipSpace(text, pos)
    while pos <= numel(text) && any(text(pos) == [' ', char(9), char(10), char(13)])
        pos = pos + 1;
    end
end

function pos = expect(text, pos, token)
    if pos > numel(text) || text(pos) ~= token
        error('bifti:json', 'Expected ''%s'' at position %d', token, pos);
    end
    pos = pos + 1;
end

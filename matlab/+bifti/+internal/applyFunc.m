function y = applyFunc(func, x)
%APPLYFUNC Apply a BIfTI "func" expression to every voxel of X.
%   The grammar of JSON.md ("Transformed reference") is parsed exactly and
%   never eval'ed, so a phantom file cannot run code:
%       expr   = term   (("+" | "-") term)*
%       term   = factor (("*" | "/") factor)*
%       factor = number | variable | "(" expr ")" | ("+" | "-") factor
%   Variables: x (the voxel value) and the volume statistics x_min, x_max,
%   x_mean, x_std (population); x_min/x_max compare real parts.
    tokens = tokenize(func);
    [ast, pos] = parseExpr(func, tokens, 1);
    if pos <= numel(tokens)
        error('bifti:func', 'Invalid func "%s": unexpected "%s"', func, tokens{pos});
    end
    variables = struct('x', x, 'x_min', min(real(x(:))), 'x_max', max(real(x(:))), ...
        'x_mean', mean(x(:)), 'x_std', std(x(:), 1));
    y = evaluate(ast, variables);
    if isscalar(y)
        % An expression without x is a constant, but the map stays a map.
        y = repmat(y, size(x));
    end
end

function tokens = tokenize(func)
    pattern = '\s*((?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?|[A-Za-z_]\w*|[-+*/()]|\S)';
    tokens = regexp(func, pattern, 'tokens');
    tokens = cellfun(@(t) t{1}, tokens, 'UniformOutput', false);
    for k = 1:numel(tokens)
        t = tokens{k};
        if any(regexp(t, '^[A-Za-z_]')) && ~any(strcmp(t, {'x', 'x_min', 'x_max', 'x_mean', 'x_std'}))
            error('bifti:func', 'Invalid func "%s": unknown variable %s', func, t);
        elseif isempty(regexp(t, '^([\d.]|[A-Za-z_]|[-+*/()])', 'once'))
            error('bifti:func', 'Invalid func "%s": unexpected character "%s"', func, t);
        end
    end
end

function [ast, pos] = parseExpr(func, tokens, pos)
    [ast, pos] = parseTerm(func, tokens, pos);
    while pos <= numel(tokens) && any(strcmp(tokens{pos}, {'+', '-'}))
        op = tokens{pos};
        [rhs, pos] = parseTerm(func, tokens, pos + 1);
        ast = {op, ast, rhs};
    end
end

function [ast, pos] = parseTerm(func, tokens, pos)
    [ast, pos] = parseFactor(func, tokens, pos);
    while pos <= numel(tokens) && any(strcmp(tokens{pos}, {'*', '/'}))
        op = tokens{pos};
        [rhs, pos] = parseFactor(func, tokens, pos + 1);
        ast = {op, ast, rhs};
    end
end

function [ast, pos] = parseFactor(func, tokens, pos)
    if pos > numel(tokens)
        error('bifti:func', 'Invalid func "%s": unexpected end', func);
    end
    token = tokens{pos};
    if any(regexp(token, '^[\d.]'))
        ast = str2double(token);
        pos = pos + 1;
    elseif any(regexp(token, '^[A-Za-z_]'))
        ast = token;
        pos = pos + 1;
    elseif strcmp(token, '(')
        [ast, pos] = parseExpr(func, tokens, pos + 1);
        if pos > numel(tokens) || ~strcmp(tokens{pos}, ')')
            error('bifti:func', 'Invalid func "%s": expected ")"', func);
        end
        pos = pos + 1;
    elseif any(strcmp(token, {'+', '-'}))
        [operand, pos] = parseFactor(func, tokens, pos + 1);
        ast = {token, 0, operand};
    else
        error('bifti:func', 'Invalid func "%s": unexpected "%s"', func, token);
    end
end

function value = evaluate(ast, variables)
    if isnumeric(ast)
        value = ast;
    elseif ischar(ast)
        value = variables.(ast);
    else
        lhs = evaluate(ast{2}, variables);
        rhs = evaluate(ast{3}, variables);
        switch ast{1}
            case '+', value = lhs + rhs;
            case '-', value = lhs - rhs;
            case '*', value = lhs .* rhs;
            case '/', value = lhs ./ rhs;
        end
    end
end

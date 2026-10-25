function save_plot_figure(fig_handle, output_dir, filename_base, format_str_input, resolution_dpi) % Renamed format_str to format_str_input
% SAVE_PLOT_FIGURE Saves a figure to a specified format and location.
%
% Args:
%   fig_handle (matlab.ui.Figure): Handle of the figure to save.
%   output_dir (string/char): Directory to save the plot.
%   filename_base (string/char): Base name for the file (without extension).
%   format_str_input (string/char): Desired format (e.g., "png", "pdf", "fig", "svg").
%   resolution_dpi (scalar, optional): Resolution in DPI for raster formats. Default 300.

    arguments
        fig_handle (1,1) matlab.ui.Figure {mustBeValidGraphicsObject_local}
        output_dir (1,1) string 
        filename_base (1,1) string
        format_str_input (1,1) string = "png" % Keep default, remove lower() from validator here
        resolution_dpi (1,1) double {mustBePositive} = 300
    end

    % Convert to lower case and validate AFTER the arguments block
    format_str = lower(format_str_input);
    allowed_formats = ["png", "jpeg", "tiff", "pdf", "eps", "fig", "svg"];
    if ~ismember(format_str, allowed_formats)
        error('grabACHAnalysisTools:save_plot_figure:InvalidFormat', ...
              'Unsupported format "%s". Allowed formats are: %s.', format_str_input, strjoin(allowed_formats, ", "));
    end

    if ~exist(output_dir, 'dir')
        try
            mkdir(output_dir);
            fprintf('Info: Created output directory: %s\n', output_dir);
        catch ME_mkdir
            warning('grabACHAnalysisTools:save_plot_figure:MkdirFailed', ...
                    'Failed to create output directory %s. Error: %s. Plot not saved.', output_dir, ME_mkdir.message);
            return;
        end
    end

    full_filename = fullfile(output_dir, filename_base + "." + format_str); % Use the lowercased format_str

    try
        switch format_str % Use the lowercased format_str
            case {"png", "jpeg", "tiff"}
                print(fig_handle, char(full_filename), ['-d' char(format_str)], ['-r' num2str(resolution_dpi)]);
            case "pdf"
                current_renderer = get(fig_handle, 'Renderer');
                if strcmp(current_renderer, 'painters')
                     print(fig_handle, char(full_filename), '-dpdf', '-painters', ['-r' num2str(resolution_dpi)]);
                else 
                     print(fig_handle, char(full_filename), '-dpdf', ['-r' num2str(resolution_dpi)]);
                end
            case "eps"
                 print(fig_handle, char(full_filename), '-depsc', '-painters'); 
            case "svg"
                 print(fig_handle, char(full_filename), '-dsvg', '-painters'); 
            case "fig"
                savefig(fig_handle, char(full_filename));
            % No 'otherwise' needed here due to prior validation
        end
        fprintf('Saved plot to: %s\n', full_filename);
    catch ME
        warning('grabACHAnalysisTools:save_plot_figure:SaveFailed', ...
                'Failed to save plot %s. Error: %s', full_filename, ME.message);
    end
end

% Local validation function for graphics handle
function mustBeValidGraphicsObject_local(a)
    if ~isa(a, 'matlab.ui.Figure') || ~isvalid(a) 
        eid = 'grabACHAnalysisTools:mustBeValidGraphicsObject_local:notValidFigure';
        msg = 'Input must be a valid MATLAB Figure handle.';
        throwAsCaller(MException(eid,msg));
    end
end
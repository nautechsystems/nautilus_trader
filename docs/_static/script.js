$(document).ready(function(){
	setmenu();

	function setmenu() {
		$("#menu .md-nav__link--active").each(function() {

			if($(this).parent().closest('.submenu').length!=0){
				$(this).children().closest('.submenu').addClass('active-submenu').slideDown(220);
				$(this).children().closest('.arrow').addClass("arrow-animate");
	
				submenus = $(this).parent().closest('.submenu');
				$(submenus).parent().closest('.md-nav__item').addClass("parent-active-menu");
				$(submenus).addClass('active-submenu').slideDown(220);
	
				submenus = $(submenus).parent().closest('.submenu');
				$(submenus).parent().closest('.md-nav__item').addClass("parent-active-menu");
				$(submenus).addClass('active-submenu').slideDown(220);
	
			} else {
				submenus = $(this).children().closest('.submenu');
				$(submenus).parent().closest('.md-nav__item').addClass("parent-active-menu");
				$(submenus).addClass('active-submenu').slideDown(220);
			}
			setarrows();
		});
	}
	
	function setarrows(){
		$(".parent-active-menu").each(function() {
			$(this).children().closest('.arrow').addClass("arrow-animate");
		});
	}

	$('.arrow').on('click', function(e) {
		if($(this).hasClass('arrow-animate')){
			$(this).removeClass('arrow-animate');
			$(this).next().slideUp(220);
		} else {
			parent=$(this).parent();
			//Main menu
			if($(parent).parent().hasClass('md-nav__list')){
				$(".arrow-animate").each(function() {
					currentParent=$(this).parent();
					if($(currentParent).parent().hasClass('md-nav__list')){
						$(this).removeClass('arrow-animate');
						$(this).next().slideUp(220);
					}
				});
			}else{
				//Submenu
				parent=$(this).closest('li').siblings().has('span');
				$(parent).children().each(function() {
					if($(this).hasClass('arrow-animate')){
						$(this).removeClass('arrow-animate');
						$(this).next().slideUp(220);
					}
				});
			}
			$(this).addClass('arrow-animate');
			$(this).next().slideDown(220);
		}
		
	});

	
}); 